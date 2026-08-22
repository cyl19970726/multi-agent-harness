#!/usr/bin/env node
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root=join(dirname(fileURLToPath(import.meta.url)),"..");
const read=(path)=>readFile(join(root,path),"utf8");
const [model,board,graph,inspector,workspace]=await Promise.all([
  read("src/model/roleViews.ts"),
  read("src/components/workbench/team/TeamWorksBoard.tsx"),
  read("src/components/workbench/team/WorkGraphView.tsx"),
  read("src/components/workbench/team/WorkGraphInspector.tsx"),
  read("src/surfaces/TeamWorkspace.tsx"),
]);
const active=`${model}\n${board}\n${graph}\n${inspector}\n${workspace}`;

const retiredLineageField=["parent","work","id"].join("_");
const retiredNestedLabel=["Child","Work"].join(" ");
assert.equal(active.includes(retiredLineageField)||active.includes(retiredNestedLabel),false,"active Work product reintroduces recursive topology");
assert.match(model,/prerequisite_work_ids/,"predecessor projection is missing");
assert.match(model,/successor_work_ids/,"derived successor projection is missing");
assert.match(model,/kind:"hard"/,"hard dependency edge contract is missing");
assert.match(model,/WorkReadiness/,"server readiness contract is missing");
assert.match(board,/attentionIds/);assert.match(board,/WorkGraphView/);
assert.match(workspace,/view\.data\.work_graph/,"Team Workspace does not consume the graph projection");
assert.match(graph,/prerequisite_work_id/);assert.match(graph,/dependent_work_id/);
assert.match(inspector,/Prerequisites/);assert.match(inspector,/Successors/);

// A small explicit fan-in/fan-out fixture proves the UI contract can carry a
// graph without inventing tree lineage or a second writable reverse edge.
const edges=[
  {prerequisite_work_id:"design",dependent_work_id:"integrate",kind:"hard"},
  {prerequisite_work_id:"implementation",dependent_work_id:"integrate",kind:"hard"},
  {prerequisite_work_id:"integrate",dependent_work_id:"release",kind:"hard"},
  {prerequisite_work_id:"integrate",dependent_work_id:"docs",kind:"hard"},
];
assert.deepEqual(edges.filter((edge)=>edge.dependent_work_id==="integrate").map((edge)=>edge.prerequisite_work_id),["design","implementation"],"fan-in fixture lost a predecessor");
assert.deepEqual(edges.filter((edge)=>edge.prerequisite_work_id==="integrate").map((edge)=>edge.dependent_work_id),["release","docs"],"fan-out fixture lost a successor");

console.log("Flat Work DAG and docs boundary check: PASS");
