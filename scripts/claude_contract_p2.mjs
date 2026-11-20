// Only adapters live here. SDK serialization, accumulation, turn shaping and
// retry predicates/delay selection are executed from checked binary ranges.
import fs from 'node:fs';
import vm from 'node:vm';
const [directory, base, upstream, casesPath] = process.argv.slice(2);
const cases = JSON.parse(fs.readFileSync(casesPath)).cases;
function check(value, message) { if (!value) throw Error(message); }
const waits = [];
const context = vm.createContext({console, Headers, TextEncoder, TextDecoder, ReadableStream,
  AbortController, Buffer, setTimeout, clearTimeout, Ao:class extends Error {},
  Jc:class extends Error {}, Zdt:e=>e?.name==='AbortError', ni:'x-stainless-helper-method',
  nd:class {constructor(){throw Error('native tool runner unavailable');}}, rb:class extends Error {}});
vm.runInContext(fs.readFileSync(directory+'/sdk-source.js','utf8')+'\n globalThis.sdk={zi,zt,ff};',context,{timeout:2000});
// The SDK runtime chunk declares Ut itself. Replace that time helper only after
// loading it; shouldRetry/retryRequest/calculateDefaultRetryTimeoutMillis stay verbatim.
context.Ut=async ms=>{check(Number.isFinite(ms)&&ms>=0&&ms<=10000,'unbounded retry delay');waits.push(ms);};
vm.runInContext(fs.readFileSync(directory+'/p2-source.js','utf8'),context,{timeout:2000});
const {zi,zt,ff}=context.sdk;
const {Retry,SXn}=context.p2;
async function localFetch(url, options={}) {
  const parsed=new URL(url);
  check([base,upstream].includes(parsed.origin)&&parsed.hostname==='127.0.0.1','non-loopback transport');
  return fetch(url,{...options,redirect:'error',signal:options.signal??AbortSignal.timeout(8000)});
}
async function control(path,body) {
  const r=await localFetch(upstream+path,{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify(body)});
  const data=await r.json();check(r.ok,data.error??'fixture control failed');return data;
}
const wires=[];
const httpErrors=new Map();
let current;
const client={_options:{timeout:8000},post(path,options){
  const headers=new Headers(options.headers?.values);
  headers.set('content-type','application/json');headers.set('x-api-key','p2-synthetic-client');
  headers.set('anthropic-version','2023-06-01');
  wires.push({id:current.id,path,body:options.body,headers:Object.fromEntries(headers)});
  const promise=localFetch(base+path,{method:'POST',headers,body:JSON.stringify(options.body),signal:options.signal})
    .then(async response=>{
      if(!response.ok){
        const detail={status:response.status,body:await response.json()};
        httpErrors.set(current.id,detail);
        throw Object.assign(Error('HTTP '+response.status),detail);
      }
      return {response,data:zt.fromSSEResponse(response,new AbortController())};
    });
  return {withResponse:()=>promise};
}};
const messages=new zi(client);
const outcomes=[];
for(const scenario of cases) {
  current=scenario;
  await control('/control',{id:scenario.id});
  const model=scenario.route==='codex'?'gpt-6.2-sol':'claude-haiku-5-5';
  let body={model,max_tokens:64,system:[{type:'text',text:'# System\nP2 base'}],
    messages:[{role:'user',content:scenario.id}],betas:['per-turn-control-2026-07-01','advanced-tool-use-2025-11-20']};
  if(scenario.family==='tools') {
    const name=scenario.tool;
    body.tools=[{name:'ToolSearch',input_schema:{type:'object',properties:{query:{type:'string'}}}},
      {name,input_schema:{type:'object',properties:{file_path:{type:'string'}},required:['file_path']},defer_loading:true}];
    body.messages.push({role:'assistant',content:[{type:'tool_use',id:'toolu_'+scenario.id,name:'ToolSearch',input:{query:'select:'+name}}]},
      {role:'user',content:[{type:'tool_result',tool_use_id:'toolu_'+scenario.id,content:[{type:'tool_reference',tool_name:name}]}]});
  }
  if(scenario.family==='effort'||scenario.family==='clear_at') {
    // The actual main-client wire shaper, not a duplicate of its field logic.
    const controls={dropBase:new Set(),clearAtAfter:new Map(),statementAfter:new Map()};
    body.messages.push({role:'assistant',content:[{type:'text',text:'previous'}]});
    if(scenario.family==='effort') {
      body.output_config={effort:'low'};controls.statementAfter.set(1,{effort:'high'});
      if(scenario.effort_only)delete body.system;
      else controls.clearAtAfter.set(1,[{type:'text',text:'P2 turn control'}]);
    } else {
      controls.clearAtAfter.set(1,[{type:'text',text:'P2 ephemeral'}]);
      if(scenario.followup)body.messages.push({role:'user',content:'P2 next user'});
    }
    body.messages=SXn(body.messages,controls).wire;
    check(body.messages[2].role==='system','main shaper control position');
    if(scenario.family==='effort') {
      const stated=body.messages.filter(m=>m.output_config?.effort!==undefined);
      check(stated.length===1&&stated[0].output_config.effort==='high'&&stated[0].content.length===0,'effective nested effort wire control');
      check(body.output_config.effort==='low','top-level effort control');
    }else{
      check(body.messages[2].clear_at==='next_user_message'&&body.messages[2].content[0].text==='P2 ephemeral','clear_at wire control');
      check(!scenario.followup||body.messages[3].content==='P2 next user','followup chronology wire control');
    }
  }
  let actual;
  if(scenario.family==='retry') {
    // Consume actual direct and gateway HTTP responses with the original SDK
    // methods. makeRequest is a counting adapter: this is NOT automatic retry.
    const path='/v1/messages?beta=true';
    body.stream=true;
    const response=await localFetch((scenario.direct?upstream:base)+path,{method:'POST',
      headers:{'content-type':'application/json','x-api-key':scenario.direct?'p2-synthetic-upstream':'p2-synthetic-client',
        'anthropic-version':'2023-06-01','anthropic-beta':body.betas.join(',')},body:JSON.stringify(body)});
    check(response.status===429,'retry response status');
    const error=await response.json();check(error.error.message==='P2 limited','retry response body');
    for(const key of ['retry-after-ms','retry-after','x-should-retry','request-id','anthropic-ratelimit-unified-status'])
      check(response.headers.has(key)===scenario.direct,'retry metadata baseline changed: '+key);
    const retry=new Retry();retry._authFlags=()=>({});retry._authState={};retry.maxRetries=2;
    let makeRequests=0;retry.makeRequest=async(_options,left)=>{makeRequests++;return {left};};
    const decision=await retry.shouldRetry(response,{});
    const start=waits.length;
    if(decision)await retry.retryRequest({maxRetries:2},2,'p2',response.headers);
    actual={decision,waits:waits.slice(start),makeRequests,status:response.status,headers:Object.fromEntries(response.headers),layer:'original shouldRetry + retryRequest; bounded wait/makeRequest adapters; no automatic transport retry'};
    const expectedDecision=scenario.direct?scenario.allow:true;
    check(decision===expectedDecision,'retry predicate mismatch');
    check(makeRequests===(decision?1:0),'retry method adapter count');
    if(decision)check(actual.waits.length===1&&(scenario.direct?actual.waits[0]===125:actual.waits[0]>=375&&actual.waits[0]<=500),'retry delay priority/default mismatch');
  } else {
    const stream=ff.createMessage(messages,body);
    let deltaSeen=false;
    stream.on('streamEvent',event=>{if(event.type==='content_block_delta'&&event.delta.type==='text_delta')deltaSeen=true;});
    const final=stream.finalMessage();
    // Prevent an unhandled rejection while waiting at the barrier.
    final.catch(()=>{});
    let beforeRelease;
    if(scenario.family==='sse') {
      const deadline=Date.now()+4000;
      while(!(await control('/state',{})).fragment_sent){check(Date.now()<deadline,'fixture fragment deadline');await new Promise(r=>setTimeout(r,10));}
      const start=Date.now();
      while(!deltaSeen&&Date.now()-start<350)await new Promise(r=>setTimeout(r,10));
      beforeRelease=deltaSeen;
      const state=await control('/state',{});check(state.closed,'terminal barrier opened prematurely');
      check(beforeRelease===(scenario.route==='anthropic'),'first delta baseline mismatch');
      await control('/release',{});
    }
    try {
      const message=await final;
      const text=message.content.filter(x=>x.type==='text').map(x=>x.text).join('');
      check(!scenario.expected_error,'expected validation error disappeared');
      check(text==='P2 terminal '+scenario.id,'eventual terminal text mismatch');
      check(message.stop_reason==='end_turn','terminal stop reason mismatch');
      actual={text,usage:message.usage,stop_reason:message.stop_reason,beforeRelease,layer:'zi.create -> zt/cc -> ff.finalMessage; SXn for turn controls'};
      if(scenario.family==='usage') {
        check(message.usage.input_tokens===(scenario.route==='codex'?0:17),'terminal input usage mismatch');
        check(message.usage.output_tokens===(scenario.route==='codex'?0:11),'terminal output usage mismatch');
        if(scenario.route==='anthropic')check(message.usage.cache_read_input_tokens===5&&message.usage.cache_creation_input_tokens===3,'terminal cache usage mismatch');
      }
    }catch(error){
      const detail=httpErrors.get(scenario.id);
      check(scenario.expected_error&&detail?.status===400&&detail.body?.type==='error',
        'unexpected SDK/gateway error '+scenario.id+': '+error.message+' '+JSON.stringify(detail));
      check(detail.body.error?.type==='invalid_request_error'&&
        detail.body.error.message==='message content array must contain at least one text item (field: messages[].content)',
        'effort-only validation error did not identify empty system content');
      actual={status:detail.status,error:detail.body,layer:'zi.create -> HTTP structured validation error (SDK wraps transport Error)'};
    }
  }
  const receipt=await control('/finish',{id:scenario.id});
  outcomes.push({id:scenario.id,family:scenario.family,route:scenario.route,
    outcome:scenario.expected_outcome,expected:scenario.expected,actual,fixture:receipt,result:0});
}
await control('/complete',{});
console.log(JSON.stringify({outcomes,wires,result:0}));
