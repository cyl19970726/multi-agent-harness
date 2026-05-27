import { UserRound } from "lucide-react";
import { byId, teamMembers } from "../readModel";
import type { DashboardSnapshot } from "../types";
import { Pill } from "./Pill";

export function TeamRoster({
  snapshot,
  onSelectMember,
}: {
  snapshot: Required<DashboardSnapshot>;
  onSelectMember: (id: string) => void;
}) {
  const membersById = byId(snapshot.members);

  return (
    <div className="teamStack">
      {snapshot.teams.map((team) => {
        const members = teamMembers(team, snapshot.members);
        return (
          <article className="teamMini" key={team.id}>
            <strong>{team.name || team.id}</strong>
            <span>owner={team.owner_agent_id || "-"}</span>
            {members.map((member) => (
              <button className="memberRow" type="button" key={member.id} onClick={() => onSelectMember(member.id)}>
                <UserRound size={13} />
                <span>{member.name || member.id}</span>
                <Pill tone={member.runtime_status === "running" && member.runtime_alive ? "good" : "warn"}>
                  {member.runtime_status || member.status || "offline"}
                </Pill>
              </button>
            ))}
            {!members.length && <span className="muted">No members</span>}
            {team.owner_agent_id && !membersById.has(team.owner_agent_id) && (
              <span className="muted">owner not registered</span>
            )}
          </article>
        );
      })}
      {!snapshot.teams.length && <div className="empty">No teams</div>}
    </div>
  );
}
