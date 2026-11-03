import { useQuery } from "@tanstack/react-query";
import { AnimatePresence, motion } from "motion/react";

import { ApprovalCard } from "@/features/chat/actions";
import { api, keys } from "@/lib/api";

/**
 * Cards someone the user trusts is waiting on, when their card in People says the user
 * approves for them. Only the card reaches the user, never that person's conversation;
 * it goes away once decided, here or from a messaging app.
 */
export function GuestApprovals() {
  const waiting = useQuery({ queryKey: keys.guestApprovals, queryFn: api.guestApprovals, retry: false });
  const list = waiting.data ?? [];
  return (
    <div className="pointer-events-none fixed top-[72px] right-4 z-40 flex w-[min(400px,calc(100vw-32px))] flex-col gap-3">
      <AnimatePresence initial={false}>
        {list.map((g) => (
          <motion.div
            key={g.action.id}
            className="pointer-events-auto"
            exit={{ opacity: 0, scale: 0.97 }}
            transition={{ type: "spring", stiffness: 420, damping: 34 }}
          >
            <ApprovalCard action={g.action} heading={`${g.name} asks for your OK`} />
          </motion.div>
        ))}
      </AnimatePresence>
    </div>
  );
}
