#!/usr/bin/env node
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root=resolve(dirname(fileURLToPath(import.meta.url)),"..");
const read=(file)=>readFile(join(root,file),"utf8");
const [model,types,board,graph,inspector,workspace]=await Promise.all([
  read("src/model/roleViews.ts"),read("src/types.ts"),read("src/components/workbench/team/TeamWorksBoard.tsx"),read("src/components/workbench/team/WorkGraphView.tsx"),read("src/components/workbench/team/WorkGraphInspector.tsx"),read("src/surfaces/TeamWorkspace.tsx"),
]);

const retiredLineageField=["parent","work","id"].join("_");
assert.equal(`${model}\n${types}\n${board}\n${graph}\n${inspector}`.includes(retiredLineageField),false,"current Dashboard retains retired tree lineage");
assert.equal(`${board}\n${graph}\n${inspector}`.includes(`>${["Par","ent"].join("")}`),false,"current Work UI renders a retired lineage fact");
for(const token of ["successor_work_ids","readiness","unsatisfied_prerequisite_work_ids","failed_or_cancelled_prerequisite_work_ids","WorkGraphEdge","ready_work_ids","attention_work_ids"]){assert.match(model,new RegExp(token),`RoleView model omits ${token}`);}
assert.match(model,/change_work_dependencies/,"authorized dependency action is missing");
assert.match(model,/action:"replace_work_dependencies"/,"dependency action intent is not closed");
assert.match(model,/works\/\$\{id\}\/dependencies/,"dependency action route is missing");
assert.match(workspace,/graph=\{view\.data\.work_graph/,"TeamWorkspace does not pass the authoritative graph");
assert.match(board,/attentionIds\.has\(work\.work_id\)/,"attention filter does not consume the authoritative attention set");
assert.match(graph,/data-work-graph-node/,"desktop graph nodes are missing");
assert.match(graph,/markerEnd="url\(#work-graph-arrow\)"/,"hard dependency edges are not rendered");
assert.match(graph,/ArrowLeft/);assert.match(graph,/ArrowRight/);assert.match(graph,/ArrowUp/);assert.match(graph,/ArrowDown/);
assert.match(graph,/team-work-graph-compact/,"compact graph fallback is missing");
assert.match(inspector,/Server-authoritative readiness/,"readiness provenance is not visible");
assert.match(inspector,/failed_or_cancelled_prerequisite_work_ids/,"failed/cancelled prerequisite attention is hidden");
assert.match(inspector,/prepareRoleAction/,"dependency editor bypasses the closed action adapter");
assert.match(inspector,/The dashboard will not infer claimability/,"missing readiness is silently inferred");
assert.doesNotMatch(board,/prerequisites_satisfied|every\([^)]*resolution|phase\s*===\s*["']closed["'].*Ready/s,"UI implements authoritative readiness logic");

console.log("Work Graph Dashboard contract check: PASS");
