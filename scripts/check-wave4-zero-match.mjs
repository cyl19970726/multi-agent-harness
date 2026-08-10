import assert from "node:assert/strict";import fs from "node:fs";import path from "node:path";
const roots=["crates","schemas","apps/agent-dashboard/src","docs",".agents/skills","plugins"];
const forbidden=[/POST\s+\/v1\/views\//i,/company[_ -]?workitem/i,/browser[_ -]?authority/i,/per[_ -]?team daemon/i];
const violations=[];
function walk(p){if(!fs.existsSync(p))return;for(const e of fs.readdirSync(p,{withFileTypes:true})){const f=path.join(p,e.name);if(e.isDirectory())walk(f);else if(/\.(rs|ts|tsx|js|mjs|json|md)$/.test(e.name)){const s=fs.readFileSync(f,"utf8");for(const rule of forbidden)if(rule.test(s))violations.push(`${f}: ${rule}`)}}}
for(const root of roots)walk(root);
// The governance check itself names retired terms as patterns; exclude it.
const actual=violations.filter(v=>!v.includes("scripts/check-wave4-zero-match.mjs")).filter(v=>{
  const file=v.split(": ")[0];
  if(!file.startsWith("docs/decisions/"))return true;
  const text=fs.readFileSync(file,"utf8").toLowerCase();
  return !(text.includes("superseded")||text.includes("removed that duplicate"));
});
assert.deepEqual(actual,[],`retired Wave4 authority matches:\n${actual.join("\n")}`);console.log("wave4 zero-match: PASS");
