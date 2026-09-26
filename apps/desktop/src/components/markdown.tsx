import { memo } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { openExternal } from "@/lib/transport";

/**
 * Renders model output. Raw HTML is not rendered, and links open in the system browser
 * rather than navigating the app's webview.
 */
export const Markdown = memo(function Markdown({ children }: { children: string }) {
  return (
    <div className="prose prose-sm prose-neutral max-w-none dark:prose-invert prose-p:leading-relaxed prose-pre:bg-muted prose-pre:text-foreground prose-code:before:content-none prose-code:after:content-none">
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
