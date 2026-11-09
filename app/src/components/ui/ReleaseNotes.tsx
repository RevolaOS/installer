// SPDX-License-Identifier: GPL-3.0-only
import type React from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { openExternalUrl } from "../../lib/backend";
import { cn } from "../../lib/cn";
import { linkLabel, tidyNotes } from "../../lib/releaseNotes";

interface IReleaseNotesProps {
  /** Markdown body of a GitHub release. */
  notes: string;
  className?: string;
}

function Link({ href = "", children }: React.ComponentProps<"a">) {
  const text = typeof children === "string" ? children : href;
  return (
    <a
      href={href}
      className="text-ink underline decoration-[#c8c3bb] underline-offset-4 hover:decoration-focus"
      onClick={(event) => {
        event.preventDefault();
        void openExternalUrl(href);
      }}
    >
      {linkLabel(href, text)}
    </a>
  );
}

const Heading = ({ children }: { children?: React.ReactNode }) => (
  <p className="mt-4 text-[11px] font-bold tracking-[0.08em] text-muted uppercase first:mt-0">
    {children}
  </p>
);

const ReleaseNotes: React.FC<IReleaseNotesProps> = ({ notes, className }) => (
  <div className={cn("text-left text-[13px] leading-6 text-body [&_p+p]:mt-2", className)}>
    <Markdown
      skipHtml
      remarkPlugins={[remarkGfm]}
      components={{
        a: Link,
        h1: Heading,
        h2: Heading,
        h3: Heading,
        h4: Heading,
        ul: ({ children }) => <ul className="mt-1 space-y-1 [&_li]:pl-4">{children}</ul>,
        ol: ({ children }) => <ol className="mt-1 space-y-1 [&_li]:pl-4">{children}</ol>,
        li: ({ children }) => (
          <li className="relative before:absolute before:left-0 before:text-muted before:content-['–']">
            {children}
          </li>
        ),
        strong: ({ children }) => <strong className="font-medium text-ink">{children}</strong>,
        code: ({ children }) => <code className="font-mono text-[12px]">{children}</code>,
        pre: ({ children }) => (
          <pre className="my-2 overflow-x-auto whitespace-pre-wrap">{children}</pre>
        ),
      }}
    >
      {tidyNotes(notes)}
    </Markdown>
  </div>
);

export { ReleaseNotes };
