# Timeline

Use `shared.md` and add:

```text
Primary request: Company Work timeline for seeing due-date collisions and cross-line delivery sequence
Header: title "Work"; active tab "Timeline"
Toolbar: range "Jul – Oct 2026"; filters "Business line", "Owner", "Milestone", "Status";
         controls "Week", "Month", "Quarter" with Month active
Main composition: fixed left ledger for WorkItem, accountable portrait, status, and Milestone;
                  right calendar grid with restrained duration bars and exact due markers
Swimlane groups: "Brand & IP", "Content", "Finance", "Product & Engineering"
Show today marker, two due-date collisions, one blocked bar, one waiting-for-approval marker,
and Milestone diamonds for "Trademark application submitted", "Launch content system",
"Company OS Work V1", and "Q3 finance close"
Right rail: "Schedule pressure" listing overdue, due this week, approval delays, and unplanned work
Constraints: readable operating timeline, not a complex task graph or dependency Gantt;
             no invented dependency arrows; durations and dates are secondary WorkItem fields
```

