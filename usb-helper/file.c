/* SPDX-License-Identifier: GPL-3.0-only */

#include "config.h"
#include "system.h"
#include "file.h"
#include "dir.h"
#include "file_internal.h"
#include "disk.h"
#include "fat.h"
#include "ata.h"
#include "errno.h"
#include "string.h"
#include "upload.h"
#include "sha256.h"

UPLOAD_CONFIG_QUALIFIER struct upload_config upload_config = {.tag = "REVOLA-UPLOAD2"};
static int fd = -1;
static bool mounted, writes_enabled, closed;
static uint64_t data_start, data_end;
static char temporary[64];
static const char temporary_prefix[] = "/.revola-";
static struct sha256 hash;

/* Enabled only after the partition layout and mounted volume agree. */
int upload_write_allowed(uint64_t sector, int count)
{
    if (!writes_enabled || count <= 0 || sector < data_start || sector >= data_end ||
        (uint64_t)count > data_end - sector) {
        UPLOAD_RESULT->rejected++;
        return 0;
    }
    return 1;
}

static bool path_valid(const char *path)
{
    if (path[0] != '/' || !path[1])
        return false;
    unsigned start = 1;
    for (unsigned i = 1; i < 192; ++i) {
        unsigned c = (unsigned char)path[i];
        if (c == '/' || !c) {
            unsigned n = i - start;
            if (!n || (n == 1 && path[start] == '.') ||
                (n == 2 && path[start] == '.' && path[start + 1] == '.') ||
                path[i - 1] == '.' || path[i - 1] == ' ')
                return false;
            if (!c)
                return true;
            start = i + 1;
        } else if (c < 32 || c > 126 || strchr("\\:*?\"<>|", c))
            return false;
    }
    return false;
}

void upload_fail(int rc)
{
    if (!UPLOAD_RESULT->rc) {
        UPLOAD_RESULT->rc = rc;
        UPLOAD_RESULT->error_no = errno;
    }
    UPLOAD_RESULT->state = 4;
}

void upload_close(void)
{
    if (closed)
        return;
    closed = true;
    if (fd >= 0) {
        int result = close(fd);
        fd = -1;
        if (result)
            upload_fail(-320);
    }
    if (mounted) {
        int result = disk_unmount_all();
        mounted = false;
        if (result != 1)
            upload_fail(-321);
    }
    writes_enabled = false;
    ata_sleepnow();
}

#define REMOVE_DEPTH 8
static char tree[256];

static bool staged_name(const char *name)
{
    size_t n = strlen(name);
    return n > sizeof(temporary_prefix) - 2 + 5 &&
           !strncasecmp(name, temporary_prefix + 1, sizeof(temporary_prefix) - 2) &&
           !strcasecmp(name + n - 5, ".part");
}

/* Append the first child of tree[length] to tree. Returns 1 for a directory, 0 for a
   file, -1 when none is left and -2 on error. The directory is closed before the
   caller deletes anything. */
static int first_child(size_t length, bool staged)
{
    DIR *dir = opendir(length ? tree : "/");
    if (!dir)
        return -2;
    int kind = -1;
    struct DIRENT *entry;
    while ((entry = readdir(dir))) {
        const char *name = entry->d_name;
        if (!strcmp(name, ".") || !strcmp(name, "..") || (staged && !staged_name(name)))
            continue;
        size_t n = strlen(name);
        if (length + 1 + n >= sizeof(tree)) {
            kind = -2;
            break;
        }
        tree[length] = '/';
        memcpy(tree + length + 1, name, n + 1);
        kind = dir_get_info(dir, entry).attribute & ATTR_DIRECTORY ? 1 : 0;
        break;
    }
    if (closedir(dir))
        kind = -2;
    return kind;
}

static int remove_tree(size_t length, unsigned depth)
{
    for (int kind; (kind = first_child(length, false)) != -1; tree[length] = 0) {
        if (kind == -2 || (kind == 1 && (depth == REMOVE_DEPTH ||
                                         remove_tree(strlen(tree), depth + 1))) ||
            (kind == 0 && remove(tree)))
            return -1;
    }
    return rmdir(tree);
}

/* Reports what was deleted in existing_files: bit 0 companion, bit 1 firmware,
   bit 2 /Revola, bit 3 staged uploads. Missing entries are not an error. */
static void remove_installation(void)
{
    static const char *const files[] = {
        "/Revola/Firmware/loader.bin",
        "/Revola/Firmware/revola.bin",
    };
    writes_enabled = true;
    for (unsigned i = 0; i < 2; ++i)
        if (file_exists(files[i])) {
            if (remove(files[i])) {
                upload_fail(-330);
                return;
            }
            STORAGE_RESULT->existing_files |= 1u << i;
        }
    strcpy(tree, "/Revola");
    if (dir_exists(tree)) {
        if (remove_tree(strlen(tree), 0)) {
            upload_fail(-331);
            return;
        }
        STORAGE_RESULT->existing_files |= 4;
    }
    for (int kind; (kind = first_child(0, true)) != -1;) {
        if (kind != 0 || remove(tree)) {
            upload_fail(-332);
            return;
        }
        STORAGE_RESULT->existing_files |= 8;
    }
}

/* Layout is checked by the helper before calling this function. */
void upload_prepare(uint64_t start, uint64_t end, unsigned bytes)
{
    char *path = UPLOAD_RESULT->path;
    for (unsigned i = 0; i < sizeof(upload_config.path); ++i)
        path[i] = upload_config.path[i];
    UPLOAD_RESULT->size = upload_config.size;
    if (!path_valid(path) ||
        !strncasecmp(path, temporary_prefix, sizeof(temporary_prefix) - 1) ||
        upload_config.size > UPLOAD_LIMIT || upload_config.overwrite > 3) {
        upload_fail(-302);
        return;
    }
    const char hex[] = "0123456789abcdef";
    const size_t prefix_bytes = sizeof(temporary_prefix) - 1;
    memcpy(temporary, temporary_prefix, prefix_bytes);
    for (unsigned i = 0; i < 16; ++i) {
        temporary[prefix_bytes + 2 * i] = hex[UPLOAD_RESULT->nonce[i] >> 4];
        temporary[prefix_bytes + 1 + 2 * i] = hex[UPLOAD_RESULT->nonce[i] & 15];
    }
    memcpy(temporary + prefix_bytes + 32, ".part", 6);
    filesystem_init();
    int mounts = disk_mount_all();
    mounted = mounts > 0;
    struct partinfo p;
    bool match = false;
    for (int i = 0; i < 4; ++i)
        if (disk_partinfo(i, &p) && (p.type == 0x0b || p.type == 0x0c) &&
            p.start == start && p.size == end - start)
            match = true;
    if (mounts != 1 || !match ||
        (unsigned)fat_get_bytes_per_sector(IF_MV(0)) != bytes) {
        upload_fail(-303);
        return;
    }
    data_start = start;
    data_end = end;
    sector_t free_kib;
    /* Removal must work on a full disk. */
    if (!fat_size(IF_MV(0, ) NULL, &free_kib) ||
        (upload_config.overwrite != 3 &&
         (uint64_t)free_kib * 1024 <
             (uint64_t)upload_config.size + fat_get_cluster_size(IF_MV(0)))) {
        upload_fail(-322);
        return;
    }
    if (upload_config.overwrite >= 2) {
        STORAGE_RESULT->start = start;
        STORAGE_RESULT->end = end;
        STORAGE_RESULT->free_bytes = (uint64_t)free_kib * 1024;
        STORAGE_RESULT->sector_bytes = bytes;
        STORAGE_RESULT->existing_files = 0;
        if (upload_config.overwrite == 2)
            STORAGE_RESULT->existing_files =
                file_exists("/Revola/Firmware/loader.bin") |
                (file_exists("/Revola/Firmware/revola.bin") << 1);
        else
            remove_installation();
        UPLOAD_RESULT->state = 4;
        return;
    }
    if (!upload_config.overwrite && file_exists(path)) {
        upload_fail(-304);
        return;
    }
    UPLOAD_RESULT->state = 1;
}

static void begin_readback(void)
{
    sha256_final(&hash, UPLOAD_RESULT->source_sha256);
    for (unsigned i = 0; i < 32; ++i)
        if (UPLOAD_RESULT->source_sha256[i] != upload_config.sha256[i]) {
            upload_fail(-305);
            return;
        }
    if (fsync(fd)) {
        upload_fail(-306);
        return;
    }
    int rc = close(fd);
    fd = -1;
    if (rc) {
        upload_fail(-307);
        return;
    }
    upload_close();
    if (UPLOAD_RESULT->rc)
        return;
    closed = false;
    int mounts = disk_mount_all();
    mounted = mounts > 0;
    if (mounts != 1) {
        upload_fail(-308);
        return;
    }
    fd = open(temporary, O_RDONLY);
    if (fd < 0 || filesize(fd) != (off_t)upload_config.size) {
        upload_fail(-309);
        return;
    }
    sha256_init(&hash);
    UPLOAD_RESULT->state = 3;
}

void upload_start(void)
{
    writes_enabled = true;
    fd = open(temporary, O_WRONLY | O_CREAT | O_EXCL, 0666);
    if (fd < 0) {
        upload_fail(-310);
        return;
    }
    sha256_init(&hash);
    if (!upload_config.size)
        begin_readback();
}

void upload_write(unsigned bytes)
{
    if (fd < 0 || bytes > UPLOAD_CHUNK ||
        bytes > upload_config.size - UPLOAD_RESULT->written) {
        upload_fail(-311);
        return;
    }
    sha256_update(&hash, UPLOAD_DATA, bytes);
    ssize_t n = write(fd, UPLOAD_DATA, bytes);
    if (n > 0)
        UPLOAD_RESULT->written += n;
    if (n != (ssize_t)bytes) {
        upload_fail(-312);
        return;
    }
    if (UPLOAD_RESULT->written == upload_config.size)
        begin_readback();
}

static int create_parents(const char *path)
{
    char parent[sizeof(upload_config.path)];
    strcpy(parent, path);
    for (char *slash = strchr(parent + 1, '/'); slash; slash = strchr(slash + 1, '/')) {
        *slash = 0;
        if (!dir_exists(parent) && mkdir(parent))
            return -1;
        *slash = '/';
    }
    return 0;
}

void upload_verify_step(void)
{
    unsigned left = upload_config.size - UPLOAD_RESULT->verified;
    unsigned count = MIN(left, UPLOAD_CHUNK);
    if (count) {
        ssize_t n = read(fd, UPLOAD_DATA, count);
        if (n != (ssize_t)count) {
            upload_fail(-313);
            return;
        }
        sha256_update(&hash, UPLOAD_DATA, count);
        UPLOAD_RESULT->verified += count;
        return;
    }
    sha256_final(&hash, UPLOAD_RESULT->disk_sha256);
    if (memcmp(UPLOAD_RESULT->disk_sha256, UPLOAD_RESULT->source_sha256, 32)) {
        upload_fail(-314);
        return;
    }
    int rc = close(fd);
    fd = -1;
    if (rc) {
        upload_fail(-315);
        return;
    }
    if (!upload_config.overwrite && file_exists(UPLOAD_RESULT->path)) {
        upload_fail(-304);
        return;
    }
    writes_enabled = true;
    if (create_parents(UPLOAD_RESULT->path) || rename(temporary, UPLOAD_RESULT->path)) {
        upload_fail(-316);
        return;
    }
    upload_close();
    UPLOAD_RESULT->state = 4;
}
