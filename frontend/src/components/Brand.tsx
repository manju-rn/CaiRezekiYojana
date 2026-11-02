/**
 * The brand: "CRY" in white on the same blue as the primary buttons, and
 * the product's name beside it. `compact` drops the name below sm, where
 * the banner is short of room.
 */
export default function Brand({ size = 'md', compact }: { size?: 'md' | 'lg'; compact?: boolean }) {
  const [badge, name] = size === 'lg' ? ['h-9 px-3 text-base', 'text-xl'] : ['h-8 px-2.5 text-sm', 'text-base']
  return (
    <span className="flex items-center gap-2">
      <span
        className={`${badge} flex items-center justify-center rounded-md bg-series-1 font-bold tracking-wide text-white`}
      >
        CRY
      </span>
      <span className={`${name} font-semibold text-ink ${compact ? 'hidden sm:block' : 'block'}`}>CaiRezekiYojana</span>
    </span>
  )
}
