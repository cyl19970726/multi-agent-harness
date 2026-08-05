/**
 * The single definition of how an automated client reaches Team War Room UI.
 *
 * Both the visual capture runner (`scripts/capture-workbench-layout-v2.mjs`)
 * and the behavioural browser check import these, so a component change that
 * moves a role, label, or test id breaks the fast `check:dashboard` run instead
 * of only surfacing later as a failed screenshot capture.
 *
 * That is not hypothetical: making the workspace tabs semantic changed them
 * from `role=button` to `role=tab`, which the capture runner queried directly
 * and which nothing in the fast suite covered.
 *
 * Rule: no Team War Room selector literal belongs in a runner. Add it here.
 */

export const teamWarRoomJourney = {
  /** Workspace switcher. Semantic tabs, so `role=tab` — never `role=button`. */
  tab: (page, name) => page.getByRole("tab", { name: new RegExp(name), exact: false }),
  tabList: (page) => page.getByRole("tablist"),

  capacityStrip: (page) => page.getByTestId("team-capacity-strip"),

  worksBoardRegion: (page) => page.getByRole("region", { name: "Shared team Works board", exact: true }),
  worksBoard: (page) => page.getByTestId("team-works-board"),
  workLanes: (page) => page.getByTestId("team-works-lanes"),
  workCards: (page) => page.locator("[data-work-card]"),
  workDetailSheet: (page) => page.getByTestId("work-detail-sheet"),
  closeWorkDetails: (page) => page.getByRole("button", { name: "Close Work details", exact: true }),

  conversation: (page) => page.getByTestId("team-conversation"),
  conversationRows: (page) => page.locator('[data-testid="team-conversation"] ol > li'),
  mailboxStrip: (page) => page.getByTestId("team-mailbox-strip"),
  mailbox: (page, participantId) => page.getByTestId(`mailbox-${participantId}`),
  mailboxOpen: (page, participantId) => page.getByTestId(`mailbox-open-${participantId}`),
  /** Provider · execution-mode · model stack line under each member mailbox. */
  mailboxProviderStack: (page, participantId) => page.getByTestId(`mailbox-provider-stack-${participantId}`),
  allActivity: (page) => page.getByRole("button", { name: "All activity", exact: true }),
  activityFilter: (page, kind) => page.getByTestId(`activity-filter-${kind}`),
  activitySearch: (page) => page.getByLabel("Search team activity"),

  membersCapacity: (page) => page.getByTestId("team-members-capacity"),

  /** Responsive context disclosure rendered by the shared FocusShell. */
  contextDisclosure: (page) => page.getByText("Context & controls", { exact: true }),
};

/**
 * Selectors the capture runner depends on at desktop, in the order its journey
 * uses them. The browser check resolves each one against a live page so a drift
 * fails fast and names the exact broken step.
 */
export const desktopJourneyContract = [
  ["Works tab", (page) => teamWarRoomJourney.tab(page, "Works")],
  ["Activity tab", (page) => teamWarRoomJourney.tab(page, "Activity")],
  ["Members tab", (page) => teamWarRoomJourney.tab(page, "Members")],
  ["Shared team Works board region", (page) => teamWarRoomJourney.worksBoardRegion(page)],
  ["Works board test id", (page) => teamWarRoomJourney.worksBoard(page)],
  ["capacity strip", (page) => teamWarRoomJourney.capacityStrip(page)],
];

/**
 * Selectors that only exist once the Activity panel is open.
 * `mailbox-*` ids are fixture-specific and supplied by the caller.
 */
export const activityJourneyContract = [
  ["conversation region", (page) => teamWarRoomJourney.conversation(page)],
  ["mailbox strip", (page) => teamWarRoomJourney.mailboxStrip(page)],
  ["All activity control", (page) => teamWarRoomJourney.allActivity(page)],
  ["messages activity filter", (page) => teamWarRoomJourney.activityFilter(page, "messages")],
  ["activity search field", (page) => teamWarRoomJourney.activitySearch(page)],
];
