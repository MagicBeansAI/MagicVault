const test=require("node:test");
const assert=require("node:assert/strict");
const fs=require("node:fs");
const vm=require("node:vm");
const path=require("node:path");
const WORKER=fs.readFileSync(path.join(__dirname,"../worker.js"),"utf8");
const ID="a".repeat(32), DOC="11111111-1111-4111-8111-111111111111", OP="22222222-2222-4222-8222-222222222222";
function event(){const listeners=[];return{addListener:fn=>listeners.push(fn),emit:(...args)=>listeners.forEach(fn=>fn(...args))};}
function fixture(){
  const sent=[];let executions=0;let permission=true;let mismatched=false;
  const port={onMessage:event(),onDisconnect:event(),postMessage:value=>sent.push(value),disconnect(){}};
  const chrome={runtime:{id:ID,getURL:file=>`chrome-extension://${ID}/${file}`,onMessage:event(),onInstalled:event(),connectNative:()=>port,openOptionsPage:()=>{}},
    action:{onClicked:event()},permissions:{contains:async()=>permission},tabs:{query:async()=>[{id:1}]},
    webNavigation:{getAllFrames:async()=>[{frameId:0,documentId:DOC,url:"https://example.com/login?token=SYNTHETIC-URL-CANARY",documentLifecycle:"active"}],
      getFrame:async()=>({documentId:DOC,url:"https://example.com/login",documentLifecycle:"active"})},
    scripting:{executeScript:async options=>{executions++;assert.equal(options.world,"ISOLATED");assert.deepEqual(Array.from(options.target.documentIds),[DOC]);assert.equal(options.target.frameIds,undefined);return[{frameId:0,documentId:mismatched?OP:DOC,result:{fields:["filled"],error:null}}];}}};
  const context=vm.createContext({chrome,URL,importScripts:()=>{},magicvaultFill:()=>{}});
  vm.runInContext(WORKER,context);
  const ui=(action,sender={id:ID,url:`chrome-extension://${ID}/options.html`})=>chrome.runtime.onMessage.emit({action},sender,()=>{});
  const connect=()=>{ui("connect");port.onMessage.emit({kind:"ready",version:1,browser_handle:OP});};
  return{sent,port,ui,connect,get executions(){return executions;},deny(){permission=false;},mismatch(){mismatched=true;}};
}
async function tick(){for(let i=0;i<12;i++)await new Promise(resolve=>setImmediate(resolve));}
function fill(){return{request_id:OP,request:{method:"fill",params:{target:{tab:"1",frame:"0",document:DOC,top_document:DOC,origin:"https://example.com",top_origin:"https://example.com",is_main_frame:true},fields:[{css:"#password",value:"SYNTHETIC-WORKER-CANARY"}]}}};}

test("discovery projects safe origins and document IDs, never query URLs",async()=>{
  const f=fixture();f.connect();f.port.onMessage.emit({request_id:OP,request:{method:"targets"}});await tick();
  assert.equal(f.sent[0].result.kind,"targets");assert.equal(f.sent[0].result.data[0].origin,"https://example.com");
  assert.ok(!JSON.stringify(f.sent).includes("CANARY"));
});
test("fill uses exact document in isolated world and only returns status",async()=>{
  const f=fixture();f.connect();const request=fill();f.port.onMessage.emit(request);await tick();
  assert.equal(f.executions,1);assert.equal(f.sent[0].result.data.fields[0],"filled");
  assert.ok(!JSON.stringify(f.sent).includes("CANARY"));assert.equal(request.request.params.fields[0].value,"");
});
test("site denial and mismatched document result never become success",async()=>{
  const denied=fixture();denied.connect();denied.deny();denied.port.onMessage.emit(fill());await tick();
  assert.equal(denied.executions,0);assert.equal(denied.sent[0].result.data.error,"permission_denied");
  const stale=fixture();stale.connect();stale.mismatch();stale.port.onMessage.emit(fill());await tick();
  assert.equal(stale.sent[0].result.data.fields[0],"uncertain");
});
test("page-origin setup messages and disconnect races cannot launch a fill",async()=>{
  const page=fixture();page.ui("connect",{id:ID,url:"https://example.com/"});page.port.onMessage.emit(fill());await tick();assert.equal(page.executions,0);
  const f=fixture();f.connect();f.port.onMessage.emit(fill());f.ui("disconnect");await tick();assert.equal(f.executions,0);assert.equal(f.sent.length,0);
});
