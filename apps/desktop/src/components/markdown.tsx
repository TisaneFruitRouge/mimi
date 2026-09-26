import { memo } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

import { openExternal } from "@/lib/transport";

/**
 * Renders model output. Raw HTML is not rendered, and links open in the system browser
 * rather than navigating the app's own view.
 */
export const Markdown = memo(function Markdown({ children }: { children: string }) {
  return (
    <div className="prose prose-neutral max-w-none text-[15px] leading-[1.65] tracking-[-0.008em] text-foreground prose-headings:font-semibold prose-headings:tracking-[-0.018em] prose-headings:text-foreground prose-h1:text-[22px] prose-h2:text-[19px] prose-h3:text-[17px] prose-p:my-3 prose-a:font-medium prose-a:text-lime-deep prose-a:decoration-lime prose-a:decoration-2 prose-a:underline-offset-[3px] prose-strong:font-semibold prose-strong:text-foreground prose-code:rounded-[6px] prose-code:bg-fill prose-code:px-1.5 prose-code:py-0.5 prose-code:font-mono prose-code:text-[0.86em] prose-code:font-normal prose-code:before:content-none prose-code:after:content-none prose-pre:rounded-[14px] prose-pre:bg-subtle prose-pre:text-foreground prose-pre:shadow-[inset_0_0_0_0.5px_rgb(0_0_0/0.06)] prose-ol:my-3 prose-ul:my-3 prose-li:my-1 prose-li:marker:text-faint prose-blockquote:border-l-lime prose-blockquote:font-normal prose-blockquote:text-muted-foreground prose-hr:border-separator prose-table:text-[14px]">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          a: ({ href, children }) => (
            <a
              href={href}
              onClick={(e) => {
                e.preventDefault();
                if (href) openExternal(href);
              }}
              className="cursor-pointer"
            >
              {children}
            </a>
          ),
        }}
      >
        {children}
      </ReactMarkdown>
    </div>
  );
});
