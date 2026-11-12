#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Offline SHA and staged-file tests against the helper's C implementation."""

import ctypes
import hashlib
import subprocess
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parents[1]

MOCK = r"""
#ifndef TEST_MOCK_H
#define TEST_MOCK_H
#include <stdint.h>
#include <stdbool.h>
#include <stdlib.h>
#include <stdio.h>
#include <string.h>
#include <stdarg.h>
#include <fcntl.h>
#include <unistd.h>
#include <sys/stat.h>
#include <errno.h>
#include <assert.h>
#include <strings.h>
#include <dirent.h>
#define MIN(a,b) ((a)<(b)?(a):(b))
#define IF_MV(...)
typedef uint64_t sector_t;
#define DIRENT dirent
#define ATTR_DIRECTORY 0x10
struct dirinfo { unsigned attribute; };
struct dirinfo dir_get_info(DIR *, struct dirent *);
struct partinfo { uint64_t start, size; unsigned type; };
void filesystem_init(void);
int disk_mount_all(void);
int disk_unmount_all(void);
bool disk_partinfo(int, struct partinfo *);
unsigned fat_get_bytes_per_sector(void);
bool fat_size(sector_t *, sector_t *);
unsigned fat_get_cluster_size(void);
void ata_sleepnow(void);
bool file_exists(const char *);
bool dir_exists(const char *);
int test_mkdir(const char *);
int test_open(const char *, int, ...);
int test_close(int);
ssize_t test_read(int, void *, size_t);
ssize_t test_write(int, const void *, size_t);
int test_fsync(int);
int test_rename(const char *, const char *);
int test_remove(const char *);
int test_rmdir(const char *);
DIR *test_opendir(const char *);
off_t ffilesize(int);
int upload_write_allowed(uint64_t, int);
#ifndef TEST_IMPLEMENTATION
#define open test_open
#define close test_close
#define read test_read
#define write test_write
#define fsync test_fsync
#define rename test_rename
#define mkdir test_mkdir
#define remove test_remove
#define rmdir test_rmdir
#define opendir test_opendir
#endif
#endif
"""
HARNESS = r"""
#define TEST_IMPLEMENTATION
#include "mock.h"
#include "upload.h"
#include "sha256.h"
uint8_t test_buffer[65536] __attribute__((aligned(32)));
struct upload_result test_result;
struct storage_result test_storage;
static int fault, mounts, sleeps, opens; static bool renamed;
static bool corrupt;
void filesystem_init(void) {}
int disk_mount_all(void) { mounts++; return fault==11 ? 0 : 1; }
int disk_unmount_all(void) { return fault==6 ? 0 : 1; }
bool disk_partinfo(int n, struct partinfo *p) { (void)n; *p=(struct partinfo){394224,1000995848,0x0c}; if(fault==10)p->size++; return true; }
unsigned fat_get_bytes_per_sector(void) {return fault==9 ? 512 : 4096;}
bool fat_size(sector_t *s, sector_t *f) {(void)s; *f=fault==8 ? 0 : 1000000; return true;}
unsigned fat_get_cluster_size(void) {return 16384;}
void ata_sleepnow(void) {sleeps++;}
bool file_exists(const char *p) {return access(p+1,F_OK)==0;}
bool dir_exists(const char *p) {struct stat s; return !stat(p+1,&s) && S_ISDIR(s.st_mode);}
int test_mkdir(const char *p) {assert(upload_write_allowed(394224,1));return mkdir(p+1,0700);}
int test_open(const char *p,int flags,...) {opens++; return open(p+1,flags,0600);}
int test_close(int fd) {int r=close(fd);return fault==5 ? -1 : r;}
ssize_t test_read(int fd,void *p,size_t n) {if(fault==18&&renamed)return -1;ssize_t r=read(fd,p,n);if(fault==1 && !corrupt && r>0){((uint8_t*)p)[0]^=1;corrupt=true;}return r;}
ssize_t test_write(int fd,const void *p,size_t n) {assert(upload_write_allowed(394224,1));if(fault==2)return -1;return write(fd,p,n);}
int test_fsync(int fd) {if(fault==3)return -1;return fsync(fd);}
int test_rename(const char *a,const char *b) {assert(upload_write_allowed(394224,1));if(fault==4)return -1;renamed=true;return rename(a+1,b+1);}
int test_remove(const char *p) {assert(upload_write_allowed(394224,1));if(fault==17&&strstr(p,"state.bin"))return -1;return unlink(p+1);}
int test_rmdir(const char *p) {assert(upload_write_allowed(394224,1));return rmdir(p+1);}
DIR *test_opendir(const char *p) {return opendir(p[1] ? p+1 : ".");}
struct dirinfo dir_get_info(DIR *d,struct dirent *e) {(void)d;return (struct dirinfo){e->d_type==DT_DIR ? ATTR_DIRECTORY : 0};}
static void close_repeatedly(void);
static void touch(const char *p) {int f=open(p,O_WRONLY|O_CREAT,0600);assert(f>=0);close(f);}
static bool present(const char *p) {return access(p,F_OK)==0;}
static void removal(void) {
    assert(test_result.state==4);assert(!test_result.rc);assert(test_storage.existing_files==(fault==14 ? 5 : fault==15 ? 0 : fault==17 ? 75 : 15));
    assert(!present("Revola/Firmware/loader.bin")&&!present("Revola/Firmware/revola.bin")&&!present("Revola/Firmware"));
    if(fault==17)assert(present("Revola/Extensions/eq/data/state.bin")&&!present("Revola/Extensions/eq/main.rvx")&&!present("Revola/Extensions/eq/manifest.bin"));
    else assert(!present("Revola"));
    assert(!present(".revola-00112233445566778899aabbccddeeff.part"));
    assert(present("iPod_Control/Music/song.m4a")&&present("unrelated-notes.part"));
    close_repeatedly();
}
off_t ffilesize(int fd) {struct stat s;assert(!fstat(fd,&s));return s.st_size;}
static void original(void) {int fd=open("os.bin",O_RDONLY);assert(fd>=0);char b[4]={0};assert(read(fd,b,4)==3);assert(!memcmp(b,"old",3));assert(!close(fd));}
static void pattern(uint32_t off,unsigned n) {for(unsigned i=0;i<n;i++)test_buffer[i]=(off+i)*37+(off+i)/11;}
static void close_repeatedly(void) {
    upload_close();
    int before=sleeps;
    upload_close();
    upload_close();
    assert(sleeps==before);
}
int main(int argc,char **argv)
{
    assert(argc==5 || argc==6);
    const char *target=argc==6 ? argv[5] : "/os.bin";
    unsigned size=strtoul(argv[1],0,10); unsigned overwrite=atoi(argv[2]); bool exists=atoi(argv[3]);fault=atoi(argv[4]);
    if(exists){int f=open("os.bin",O_WRONLY|O_CREAT,0600);assert(write(f,"old",3)==3);close(f);}
    if(overwrite==3 && fault!=14 && fault!=15){ /* fault 17: state.bin refuses to go */
        assert(!mkdir("Revola",0700)&&!mkdir("Revola/Firmware",0700));
        touch("Revola/Firmware/loader.bin");touch("Revola/Firmware/revola.bin");touch(".revola-00112233445566778899aabbccddeeff.part");touch("unrelated-notes.part");
        assert(!mkdir("Revola/Extensions",0700)&&!mkdir("Revola/Extensions/eq",0700)&&!mkdir("Revola/Extensions/eq/data",0700));
        touch("Revola/Extensions/eq/manifest.bin");touch("Revola/Extensions/eq/main.rvx");touch("Revola/Extensions/eq/data/state.bin");
        assert(!mkdir("iPod_Control",0700)&&!mkdir("iPod_Control/Music",0700));touch("iPod_Control/Music/song.m4a");
    }
    if(overwrite==3 && (fault==14 || fault==15)) {
        if(fault==14){assert(!mkdir("Revola",0700)&&!mkdir("Revola/Firmware",0700));touch("Revola/Firmware/loader.bin");}
        assert(!mkdir("iPod_Control",0700)&&!mkdir("iPod_Control/Music",0700));touch("iPod_Control/Music/song.m4a");touch("unrelated-notes.part");
    }
    upload_config.size=size;upload_config.overwrite=overwrite;strcpy(upload_config.path,target);
    struct sha256 hash;sha256_init(&hash);
    for(unsigned off=0;off<size;){unsigned n=MIN(size-off,65536);pattern(off,n);sha256_update(&hash,test_buffer,n);off+=n;}
    sha256_final(&hash,upload_config.sha256);
    if(fault==7)upload_config.sha256[0]^=1;
    memset(test_result.nonce,7,16);
    upload_prepare(394224,1001390072,4096);
    if(fault>=9 && fault<=11){assert(test_result.rc==-303);assert(opens==0);assert(!upload_write_allowed(394224,1));upload_close();return 0;}
    if(overwrite==3 && (fault==0 || fault==8 || fault==14 || fault==15 || fault==17)){removal();return 0;}
    if(fault==8){assert(test_result.rc==-322);assert(opens==0);upload_close();return 0;}
    if(overwrite==2){assert(test_result.state==4);assert(!test_result.rc);assert(opens==0);assert(test_storage.existing_files==(fault==16 ? 3 : 0));assert(test_storage.start==394224);assert(test_storage.end==1001390072);assert(test_storage.free_bytes==1024000000);assert(!upload_write_allowed(394224,1));close_repeatedly();assert(sleeps==1);return 0;}
    if(exists&&!overwrite){assert(test_result.rc==-304);assert(opens==0);original();upload_close();return 0;}
    assert(test_result.state==1);test_result.state=2;upload_start();
    if(fault==12){
        assert(upload_write_allowed(394224,1));
        assert(upload_write_allowed(1001390071,1));
        assert(!upload_write_allowed(394223,1));
        assert(!upload_write_allowed(1001390072,1));
        assert(!upload_write_allowed(1001390071,2));
        assert(!upload_write_allowed(394224,0));
        assert(!upload_write_allowed(394224,-1));
        upload_close();return 0;
    }
    for(unsigned off=0;off<size&&test_result.state==2;){
        unsigned n=MIN(size-off,65536);pattern(off,n);test_result.received+=n;upload_write(n);off+=n;
        if(exists)original();
    }
    while(test_result.state==3){if(exists)original();upload_verify_step();}
    close_repeatedly();
    assert(test_result.state==4);
    if(fault==18){assert(test_result.rc==-317);assert(present("os.bin"));return 0;}
    if(fault){assert(test_result.rc);if(exists)original();return 0;}
    assert(!test_result.rc);assert(!test_result.rejected);
    assert(test_result.received==size && test_result.written==size && test_result.verified==size);
    assert(!memcmp(test_result.source_sha256,upload_config.sha256,32));
    assert(!memcmp(test_result.disk_sha256,upload_config.sha256,32));
    assert(mounts==2 && sleeps==2);
    int f=open(target+1,O_RDONLY);assert(f>=0);assert(ffilesize(f)==size);
    uint8_t readback[65536];
    for(unsigned off=0;off<size;){unsigned n=MIN(size-off,65536);assert(read(f,readback,n)==n);pattern(off,n);assert(!memcmp(test_buffer,readback,n));off+=n;}
    close(f);
    assert(!upload_write_allowed(394224,1));
    return 0;
}
"""


class SHA(ctypes.Structure):
    _fields_ = [
        ("h", ctypes.c_uint32 * 8),
        ("bytes", ctypes.c_uint64),
        ("tail", ctypes.c_uint8 * 64),
    ]


class HelperTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory()
        cls.root = Path(cls.tmp.name)
        p = cls.root
        subprocess.run(
            [
                "cc",
                "-O2",
                "-Wall",
                "-Wextra",
                "-Werror",
                "-shared",
                "-fPIC",
                str(HERE / "sha256.c"),
                "-o",
                str(p / "sha.so"),
            ],
            check=True,
        )
        cls.sha = ctypes.CDLL(str(p / "sha.so"))
        (p / "mock.h").write_text(MOCK)
        for name in [
            "config.h",
            "system.h",
            "file.h",
            "dir.h",
            "file_internal.h",
            "disk.h",
            "fat.h",
            "ata.h",
        ]:
            (p / name).write_text('#include "mock.h"\n')
        (p / "test_memory.h").write_text("""
#define UPLOAD_CONFIG_QUALIFIER
extern uint8_t test_buffer[65536] __attribute__((aligned(32)));
extern struct upload_result test_result;
extern struct storage_result test_storage;
#define UPLOAD_DATA test_buffer
#define UPLOAD_RESULT (&test_result)
#define STORAGE_RESULT (&test_storage)
""")
        (p / "harness.c").write_text(HARNESS)
        subprocess.run(
            [
                "cc",
                "-O2",
                "-Wall",
                "-Wextra",
                "-Werror",
                "-I" + str(p),
                "-I" + str(HERE),
                str(p / "harness.c"),
                "-DUPLOAD_TEST_MEMORY",
                str(HERE / "file.c"),
                str(HERE / "sha256.c"),
                "-o",
                str(p / "file-test"),
            ],
            check=True,
        )

    @classmethod
    def tearDownClass(cls):
        cls.tmp.cleanup()

    def test_incremental_sha(self):
        for size in [
            0,
            1,
            55,
            56,
            63,
            64,
            65,
            119,
            120,
            127,
            128,
            65535,
            65536,
            65537,
            13 * 1024 * 1024 + 17,
        ]:
            data = (bytes(range(256)) * (size // 256 + 1))[:size]
            for chunk in [1, 63, 512, 65536] if size < 100000 else [65536]:
                s = SHA()
                self.sha.sha256_init(ctypes.byref(s))
                for off in range(0, size, chunk):
                    part = data[off : off + chunk]
                    self.sha.sha256_update(
                        ctypes.byref(s),
                        ctypes.c_char_p(part),
                        ctypes.c_size_t(len(part)),
                    )
                out = ctypes.create_string_buffer(32)
                self.sha.sha256_final(ctypes.byref(s), out)
                self.assertEqual(out.raw, hashlib.sha256(data).digest(), (size, chunk))

    def run_file(self, size, overwrite, exists, fault, path="/os.bin", setup=None):
        with tempfile.TemporaryDirectory(dir=self.root) as directory:
            if setup:
                setup(Path(directory))
            subprocess.run(
                [
                    str(self.root / "file-test"),
                    str(size),
                    str(int(overwrite)),
                    str(int(exists)),
                    str(fault),
                    path,
                ],
                cwd=directory,
                check=True,
            )

    def test_firmware_upload_creates_directories_and_replaces_files(self):
        for name in ("revola.bin", "loader.bin"):
            path = f"/Revola/Firmware/{name}"

            def existing_directory(root):
                (root / "Revola").mkdir()

            def existing_file(root):
                destination = root / path.lstrip("/")
                destination.parent.mkdir(parents=True)
                destination.write_bytes(b"old")

            for setup in (None, existing_directory, existing_file):
                with self.subTest(name=name, setup=setup):
                    self.run_file(65537, True, False, 0, path, setup)

    def test_blocked_firmware_directory_fails_cleanly(self):
        for blocked in ("Revola", "Revola/Firmware"):

            def setup(root):
                path = root / blocked
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(b"unrelated")

            with self.subTest(blocked=blocked):
                self.run_file(
                    65537,
                    True,
                    False,
                    13,
                    "/Revola/Firmware/revola.bin",
                    setup,
                )

    def test_storage_inspection_is_read_only(self):
        self.run_file(12000000, 2, True, 0)

    def test_storage_inspection_finds_nested_firmware_files(self):
        def setup(root):
            folder = root / "Revola/Firmware"
            folder.mkdir(parents=True)
            for name in ("revola.bin", "loader.bin"):
                (folder / name).write_bytes(b"installed")

        self.run_file(12000000, 2, False, 16, setup=setup)

    def test_removal_deletes_only_revola_files(self):
        for fault in [0, 8, 14, 15, 17]:
            with self.subTest(fault=fault):
                self.run_file(0, 3, False, fault)

    def test_mount_must_match_validated_geometry_before_writes(self):
        for fault in [9, 10, 11]:
            with self.subTest(fault=fault):
                self.run_file(65537, True, True, fault)

    def test_writes_stay_inside_validated_partition(self):
        self.run_file(65537, True, True, 12)

    def test_sizes_and_replacement(self):
        self.run_file(65537, False, True, 0)
        for size in [0, 1, 65535, 65536, 65537, 13 * 1024 * 1024 + 17]:
            for overwrite, exists in [(False, False), (True, True)]:
                with self.subTest(size=size, overwrite=overwrite, exists=exists):
                    self.run_file(size, overwrite, exists, 0)

    def test_destination_readback_failure_is_reported(self):
        self.run_file(70000, 1, False, 18)

    def test_failures_preserve_existing_file(self):
        for fault in range(1, 9):
            with self.subTest(fault=fault):
                self.run_file(65537, True, True, fault)


if __name__ == "__main__":
    unittest.main()
