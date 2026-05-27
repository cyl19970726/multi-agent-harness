import { UserRound } from "lucide-react";
import { byId, teamMembers } from "../readModel";
import type { AgentMember, AgentTeam } from "../types";
import { Pill } from "./Pill";

export function TeamRoster({
  teams,
  members,
  onSelectMember,
}: {
  teams: AgentTeam[];
  members: AgentMember[];
  onSelectMember: (id: string) => void;
}) {
  const membersById = byId(members);

  return (
    <div className="teamStack">
      {teams.map((team) => {
        const teamMemberItems = teamMembers(team, members);
        return (
          <article className="teamMini" key={team.id}>
            <strong>{team.name || team.id}</strong>
            <span>owner={team.owner_agent_id || "-"}</span>
            {teamMemberItems.map((member) => (
              <button className="memberRow" type="button" key={member.id} onClick={() => onSelectMember(member.id)}>
                <UserRound size={13} />
                <span>{member.name || member.id}</span>
                <Pill tone={member.runtime_status === "running" && member.runtime_alive ? "good" : "warn"}>
                  {member.runtime_status || member.status || "offline"}
                </Pill>
              </button>
            ))}
            {!teamMemberItems.length && <span className="muted">No members in selected goal</span>}
            {team.owner_agent_id && !membersById.has(team.owner_agent_id) && (
              <span className="muted">owner not registered</span>
            )}
          </article>
        );
      })}
      {!teams.length && <div className="empty">No teams in selected goal</div>}
    </div>
  );
}
