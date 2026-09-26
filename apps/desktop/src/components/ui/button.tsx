import * as React from "react"
import { cva, type VariantProps } from "class-variance-authority"
import { cn } from "cn"
import { Slot } from "radix-ui"

const buttonVariants = cva(
  "inline-flex shrink-0 items-center justify-center gap-1.5 rounded-[10px] text-[14px] font-medium tracking-[-0.006em] whitespace-nowrap outline-none transition-[transform,background-color,color,box-shadow,filter] duration-150 active:scale-[0.97] focus-visible:ring-[3px] focus-visible:ring-ring/45 disabled:pointer-events-none disabled:opacity-45 aria-invalid:ring-destructive/20 [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4",
  {
    variants: {
      variant: {
        default:
          "bg-primary text-primary-foreground shadow-[0_1px_1px_rgb(0_0_0/0.12),inset_0_0.5px_0_rgb(255_255_255/0.12)] hover:bg-primary/88",
        lime: "bg-lime text-lime-ink shadow-[inset_0_0_0_0.5px_rgb(0_0_0/0.08),0_1px_1px_rgb(86_118_13/0.18)] hover:brightness-[0.97]",
        destructive: "bg-destructive text-white hover:bg-destructive/90",
        outline:
          "bg-background shadow-[0_0_0_0.5px_rgb(0_0_0/0.12),0_1px_2px_rgb(0_0_0/0.05)] hover:bg-subtle",
        secondary: "bg-fill text-foreground hover:bg-[rgb(118_118_128/0.18)]",
        ghost: "text-foreground hover:bg-fill",
        link: "text-lime-deep underline-offset-4 hover:underline active:scale-100",
      },
      size: {
        default: "h-9 px-4 has-[>svg]:px-3.5",
        xs: "h-6 gap-1 rounded-[7px] px-2 text-[12px] has-[>svg]:px-1.5 [&_svg:not([class*='size-'])]:size-3",
        sm: "h-8 gap-1.5 rounded-[9px] px-3 text-[13px] has-[>svg]:px-2.5",
        lg: "h-11 rounded-[12px] px-6 text-[15px] has-[>svg]:px-5",
        icon: "size-9",
        "icon-xs": "size-6 rounded-[7px] [&_svg:not([class*='size-'])]:size-3.5",
        "icon-sm": "size-8 rounded-[9px]",
        "icon-lg": "size-10 rounded-[12px]",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  }
)

function Button({
  className,
  variant = "default",
  size = "default",
  asChild = false,
  ...props
}: React.ComponentProps<"button"> &
  VariantProps<typeof buttonVariants> & {
    asChild?: boolean
  }) {
  const Comp = asChild ? Slot.Root : "button"

  return (
    <Comp
      data-slot="button"
      data-variant={variant}
      data-size={size}
      className={cn(buttonVariants({ variant, size, className }))}
      {...props}
    />
  )
}

export { Button, buttonVariants }
