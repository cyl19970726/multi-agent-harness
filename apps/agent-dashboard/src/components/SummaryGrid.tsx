interface SummaryGridProps {
  items: Array<{ label: string; value: number | string; tone?: "normal" | "warn" | "bad" }>;
}

export function SummaryGrid({ items }: SummaryGridProps) {
  return (
    <section className="summaryGrid" aria-label="Summary">
      {items.map((item) => (
        <div className={`metric ${item.tone ?? "normal"}`} key={item.label}>
          <span>{item.value}</span>
          <label>{item.label}</label>
        </div>
      ))}
    </section>
  );
}
