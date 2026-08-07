# Standing Agent workspace interaction contract

- The agent title and portrait identify a durable Organization actor, never a
  provider session or an Agent Team participation.
- Selecting a WorkItem opens `surface=work&workItem=<id>`; browser Back restores
  the same Standing Agent and workspace state.
- Selecting a source or maintained Document opens
  `surface=docs&document=<id>`; browser Back restores context.
- WorkItem links are keyboard reachable and activate with Enter.
- Desktop shows the context rail beside the activity column. Tablet and mobile
  expose the same modules through “Context & controls”; mobile uses a bottom
  sheet and does not introduce horizontal page scrolling.
- The message composer remains disabled with a visible reason until a governed
  Standing Agent command transport exists.
- Persisted or replayable thinking is forbidden. The activity stream contains
  only explicit Assignment, WorkItem, decision, evidence and Document records.
- Reduced motion must preserve every state and navigation path.
