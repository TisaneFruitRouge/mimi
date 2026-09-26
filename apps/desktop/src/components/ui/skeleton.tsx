import { cn } from "cn"

function Skeleton({ className, ...props }: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="skeleton"
      className={cn("animate-pulse rounded-[12px] bg-[rgb(118_118_128/0.1)]", className)}
      {...props}
    />
  )
}

export { Skeleton }
