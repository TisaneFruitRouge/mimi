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
    <div className="prose prose-neutral max-w-none text-[15px] leading-[1.7] text-[#2a2c31] prose-headings:font-semibold prose-headings:tracking-tight prose-headings:text-foreground prose-p:my-3 prose-a:font-medium prose-a:text-[#4d6b00] prose-a:decoration-lime prose-a:decoration-2 prose-a:underline-offset-2 prose-strong:text-foreground prose-code:rounded-md prose-code:bg-subtle prose-code:px-1.5 prose-code:py-0.5 prose-code:font-mono prose-code:text-[0.86em] prose-code:font-normal prose-code:before:content-none prose-code:after:content-none prose-pre:rounded-xl prose-pre:border prose-pre:bg-subtle prose-pre:text-foreground prose-li:my-1 prose-hr:border-border">
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
