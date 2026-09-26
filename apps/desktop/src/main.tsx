import React from "react";
import ReactDOM from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import "@fontsource-variable/geist";
import "@fontsource-variable/geist-mono";

import App from "./App";
import { Toaster } from "@/components/ui/sonner";
import { TooltipProvider } from "@/components/ui/tooltip";
import { useDaemonSync } from "@/lib/events";
import "./index.css";

// Daemon events keep the cache fresh, so refetching on focus is unnecessary.
const queryClient = new QueryClient({
  defaultOptions: { queries: { refetchOnWindowFocus: false, retry: 1 } },
});

function Root() {
  useDaemonSync(queryClient);
  return <App />;
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <TooltipProvider delayDuration={300}>
        <Root />
        <Toaster position="bottom-right" />
      </TooltipProvider>
    </QueryClientProvider>
  </React.StrictMode>,
);
