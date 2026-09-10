import type { WorkSummary } from "./roleViews";

export type WorkLens = "current" | "open" | "active" | "review" | "closed" | "eligible";

export function workMatchesLens(work:WorkSummary,lens:WorkLens,memberId:string){
  const owned=work.owner_actor_ref?.id===memberId;
  return lens==="current"?owned&&work.phase!=="closed":lens==="eligible"?!owned&&work.eligible_member_ids.includes(memberId):work.phase===lens;
}

export function reconcileWorkLens(lens:WorkLens,work:WorkSummary|undefined,memberId:string):WorkLens{
  return work&&workMatchesLens(work,lens,memberId)?lens:linkedWorkLens(work,memberId);
}

export function resolveWorkspaceWork(works:WorkSummary[],requestedId:string|undefined,currentId:string|null){
  return works.find(work=>work.work_id===(requestedId??currentId));
}

export function linkedWorkLens(work:WorkSummary|undefined,memberId:string):WorkLens{
  if(!work)return "current";
  if(work.phase!=="closed"&&work.owner_actor_ref?.id===memberId)return "current";
  switch(work.phase){
    case "open": case "active": case "review": case "closed":return work.phase;
    default:return "current";
  }
}
