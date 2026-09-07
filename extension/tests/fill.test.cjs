// Synthetic fixed-function coverage. Does not qualify real browser DOM behavior.
const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const vm = require("node:vm");
const path = require("node:path");
const SOURCE = fs.readFileSync(path.join(__dirname,"../../magicvault-effect/src/fill.js"),"utf8");
const CANARY = "SYNTHETIC-JS-FILL-CANARY";

function fixture() {
  class Input {
    constructor(type="password") { this.type=type;this.isConnected=true;this.disabled=false;this.readOnly=false;this.maxLength=-1;this._value="";this.events=[]; }
    get value(){return this._value;}
    set value(value){this._value=value;}
    getClientRects(){return this.hidden?[]:[{}];}
    closest(){return this.inert?{}:null;}
    matches(){return this.disabled || this.disabledByFieldset;}
    dispatchEvent(event){this.events.push(event.type);if(this.handler)this.handler(event);return true;}
  }
  const password=new Input();const username=new Input("text");
  const selectors={"#password":[password],"#username":[username]};
  const context=vm.createContext({HTMLInputElement:Input,Event:class{constructor(type){this.type=type;}},
    origin:"https://example.com",location:{origin:"https://example.com"},getComputedStyle:()=>({visibility:"visible"}),
    document:{defaultView:{},prerendering:false,querySelectorAll:css=>{if(css==="[")throw new Error(CANARY);return selectors[css]||[];}}});
  vm.runInContext(SOURCE,context);
  const fill=fields=>JSON.parse(JSON.stringify(context.magicvaultFill("https://example.com",fields)));
  return {password,username,selectors,context,fill};
}

test("native setter plus events updates ordinary and reactive inputs without submitting",()=>{
  const f=fixture();let tracked="";
  Object.defineProperty(f.password,"value",{get(){return this._value;},set(value){tracked=value;this._value=value;}});
  f.password.handler=()=>{tracked=f.password._value;};
  const fields=[{css:"#username",value:CANARY},{css:"#password",value:CANARY}];
  const result=f.fill(fields);
  assert.deepEqual(result,{fields:["filled","filled"],error:null});assert.equal(tracked,CANARY);
  assert.deepEqual(f.password.events,["input","change"]);assert.equal(f.password.value,CANARY);
  assert.ok(fields.every(field=>field.value===""));assert.ok(!JSON.stringify(result).includes(CANARY));
});

test("ambiguous and invalid locators refuse all writes before the first field",()=>{
  for(const css of ["#missing","[","#ambiguous"]){
    const f=fixture();f.selectors["#ambiguous"]=[f.password,f.username];
    const result=f.fill([{css:"#password",value:CANARY},{css,value:CANARY}]);
    assert.deepEqual(result.fields,["not_filled","not_filled"]);assert.notEqual(result.error,null);
    assert.equal(f.password.value,"");assert.ok(!JSON.stringify(result).includes(CANARY));
  }
});

test("hidden readonly disabled and unsupported controls are not guessed",()=>{
  for(const property of ["hidden","readOnly","disabled","disabledByFieldset","inert"]){
    const f=fixture();f.password[property]=true;
    assert.equal(f.fill([{css:"#password",value:CANARY}]).error,"unsupported_target");
    assert.equal(f.password.value,"");
  }
  const f=fixture();f.password.type="file";
  assert.equal(f.fill([{css:"#password",value:CANARY}]).error,"unsupported_target");
});

test("field replacement during input reports partial delivery without filling replacement",()=>{
  const f=fixture();f.password.handler=()=>{f.username.isConnected=false;};
  const result=f.fill([{css:"#password",value:CANARY},{css:"#username",value:CANARY}]);
  assert.deepEqual(result,{fields:["filled","not_filled"],error:"stale_target"});assert.equal(f.username.value,"");
});

test("origin mismatch and exceptions never echo material or raw diagnostics",()=>{
  const f=fixture();f.context.location.origin="https://other.example";
  const result=f.fill([{css:"#password",value:CANARY}]);
  assert.equal(result.error,"stale_target");assert.equal(f.password.value,"");
  assert.ok(!JSON.stringify(result).includes(CANARY));
});

test("same node through two different selectors is rejected before writes",()=>{
  const f=fixture();f.selectors["input"]=[f.password];
  assert.equal(f.fill([{css:"#password",value:CANARY},{css:"input",value:CANARY}]).error,"ambiguous_target");
  assert.equal(f.password.value,"");
});

test("an opaque security origin is refused even when the URL origin matches",()=>{
  const f=fixture();f.context.origin="null";
  const result=f.fill([{css:"#password",value:CANARY}]);
  assert.equal(result.error,"stale_target");assert.equal(f.password.value,"");
});

test("earlier event handlers cannot make a later field unsupported and still receive a value",()=>{
  for(const mutate of [node=>{node.type="file";},node=>{node.hidden=true;},node=>{node.maxLength=1;},node=>{node.disabledByFieldset=true;}]){
    const f=fixture();f.password.handler=()=>mutate(f.username);
    const result=f.fill([{css:"#password",value:CANARY},{css:"#username",value:CANARY}]);
    assert.deepEqual(result,{fields:["filled","not_filled"],error:"unsupported_target"});
    assert.equal(f.username.value,"");assert.ok(!JSON.stringify(result).includes(CANARY));
  }
});
