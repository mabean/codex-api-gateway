// Bounded adapters around fingerprinted original client code. Never runs the CLI.
import fs from 'node:fs';
import vm from 'node:vm';
const [directory,base,upstream,casesPath]=process.argv.slice(2);
const cases=JSON.parse(fs.readFileSync(casesPath)).cases;
const ranges=JSON.parse(fs.readFileSync(directory+'/p3-ranges.json'));
const check=(v,m)=>{if(!v)throw Error(m);};
const equal=(a,b,m)=>check(JSON.stringify(a)===JSON.stringify(b),m+' '+JSON.stringify({actual:a,expected:b}));
// Structural equality ignores object key order, never array order or new fields.
function canonical(x){return Array.isArray(x)?x.map(canonical):x&&typeof x==='object'?Object.fromEntries(Object.keys(x).sort().map(k=>[k,canonical(x[k])])):x;}
const structure=(a,b,m)=>equal(canonical(a),canonical(b),m);
const waits=[];
const context=vm.createContext({console,Headers,TextEncoder,TextDecoder,ReadableStream,AbortController,Buffer,setTimeout,clearTimeout,
  Ao:class extends Error{},Jc:class extends Error{},Zdt:e=>e?.name==='AbortError',ni:'x-stainless-helper-method',
  nd:class{constructor(){throw Error('native tool runner unavailable');}},rb:class extends Error{},
  Fs:(_name,n)=>n,lv:()=>({info(){}}),qt(){},$t(){},Gu:async body=>body?.cancel(),qu(){},
  Lzn:e=>e,Ns:JSON.parse,Error});
vm.runInContext(fs.readFileSync(directory+'/sdk-source.js','utf8')+'\nglobalThis.sdk={zi,zt,ff};',context,{timeout:2000});
context.Ut=async ms=>{check(Number.isFinite(ms)&&ms>=0&&ms<=10000,'unbounded delay');waits.push(ms);};
vm.runInContext('class Retry {'+ranges.makeRequest+ranges.retry+'}\n'+ranges.turn+'\nglobalThis.p3={Retry,SXn};',context,{timeout:2000});
const {zi,zt,ff}=context.sdk;
const {Retry,SXn}=context.p3;
const beta='context-management-2025-06-27';
const keep='advanced-tool-use-2025-11-20';
// Controlled beta registry and non-context side-effect hooks. The error matcher,
// vSt decision, lUo closure and request-field expression remain original bytes.
class APIError extends Error{constructor(status,message){super(message);this.status=status;}}
Object.assign(context,{Nt:APIError,gH:()=>false,MN:()=>false,uws:new Set(),ESt:new Map(),ads:()=>false,
  FBt:e=>e.message,WN:()=>false,iC:()=>false,
  qU:{},h8e:{},_Et:{},kse:undefined,m(){},io(){},a(){},abe:x=>x,Kt:x=>x,_C:()=> 'firstParty',ie:{model:'claude-haiku-5-5'},i:e=>{throw e;}});
vm.runInContext(ranges.rejectionRegex+';\n'+ranges.rejection+'\nglobalThis.resetContext=()=>{globalThis.Pn=[{header:"'+beta+'",name:"context_management"},{header:"'+keep+'",name:"tools"}];globalThis.TWi=Pn;globalThis.hw=Pn.map(x=>x.header);globalThis.A3=[];};\n'+
  'globalThis.heal='+ranges.contextRemoval.slice('lUo='.length)+';\nglobalThis.contextBody=(value)=>{let zq=value,Og=true,za=Pn,xpe=TWi[0];return {...('+ranges.contextGate+')}};\n'+
  'globalThis.remainingBetas=()=>Pn.map(x=>x.header);',context,{timeout:2000});
async function localFetch(url,options={}){
  const parsed=new URL(url);check([base,upstream].includes(parsed.origin)&&parsed.hostname==='127.0.0.1','non-loopback transport');
  return fetch(url,{...options,redirect:'error',signal:options.signal??AbortSignal.timeout(8000)});
}
async function control(path,body={}){
  const r=await localFetch(upstream+path,{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify(body)});
  check(r.ok,'fixture control status');return r.json();
}
let redirectRejected=false;
try{await localFetch(upstream+'/redirect');}catch{redirectRejected=true;}
check(redirectRejected,'external redirect was followed');
let current;
const wires=[],responses=[];
function sdkClient(){
  const retry=new Retry();retry.maxRetries=2;retry._requestAuthFlags=new Map();retry.middleware=[];
  retry._authFlags=()=>({});retry._authState={};retry.isStreamBody=()=>false;retry.prepareOptions=async()=>{};
  retry.buildRequest=async options=>({url:base+options.path,req:{method:'POST',headers:options.headers,body:JSON.stringify(options.body)},timeout:8000});
  retry.fetchWithTimeout=async(url,req)=>{const r=await localFetch(url,req);responses.push({id:current.id,status:r.status,headers:Object.fromEntries(r.headers),
    rawBody:r.headers.get('content-type')?.includes('text/event-stream')?undefined:await r.clone().text()});return r;};
  retry.makeStatusError=(status,data,_text,headers)=>Object.assign(new APIError(status,data?.error?.message??'HTTP error'),{data,headers});
  return {_options:{timeout:8000},post(path,options){
    const headers=new Headers(options.headers?.values);
    headers.set('content-type','application/json');headers.set('anthropic-version','2023-06-01');
    headers.set('x-api-key',current.auth==='oauth'?'sk-ant-oat01-p3-synthetic':'p3-synthetic-api-key');
    wires.push({id:current.id,path,body:options.body,headers:Object.fromEntries(headers)});
    const raw=retry.makeRequest({...options,path,headers,maxRetries:2}).then(value=>({...value,
      data:options.body.stream?zt.fromSSEResponse(value.response,new AbortController()):value.response.json()}));
    const promise=raw.then(async value=>await value.data);
    promise.catch(()=>{});
    promise.withResponse=()=>raw.then(async value=>({...value,data:await value.data}));
    return promise;
  }};
}
const outcomes=[];
for(const scenario of cases){
  current=scenario;
  await control('/control',scenario);
  const startWires=wires.length,startResponses=responses.length,startWaits=waits.length;
  let actual={},failure;
  try{
    const messages=new zi(sdkClient());
    let body={model:'haiku',max_tokens:64,system:[{type:'text',text:'x-anthropic-billing-header: cc_version=2.1.329.fixture;',cache_control:{type:'ephemeral'}},
      {type:'text',text:'P3 top',cache_control:{type:'ephemeral'}}],messages:[{role:'user',content:scenario.id}],betas:[beta,keep]};
    if(scenario.family==='fidelity'){
      body.output_config={effort:'low',format:{type:'json_schema',schema:{type:'object'}}};
      body.tools=[{name:'ToolSearch',input_schema:{type:'object'}},{name:'Read',input_schema:{type:'object',required:['file_path']},defer_loading:true}];
      body.messages.push({role:'assistant',content:[{type:'tool_use',id:'toolu_p3',name:'ToolSearch',input:{query:'select:Read'}}]},
        {role:'user',content:[{type:'tool_result',tool_use_id:'toolu_p3',content:[{type:'tool_reference',tool_name:'Read'}]}]});
      const controls={dropBase:new Set(),clearAtAfter:new Map([[1,[{type:'text',text:'P3 scoped',cache_control:{type:'ephemeral'}}]]]),statementAfter:new Map([[1,{effort:'high'}]])};
      body.messages=SXn(body.messages,controls).wire;
      check(body.messages.some(m=>m.clear_at==='next_user_message'),'SXn clear_at not executed');
      check(body.messages.some(m=>m.output_config?.effort==='high'),'SXn nested effort not executed');
      if(scenario.followup)body.messages.push({role:'user',content:'P3 subsequent user'});
    }
    if(scenario.family==='catalog'){
      const r=await localFetch(base+'/v1/models?limit=1000');const data=await r.json();
      check(r.ok&&data.data.some(x=>x.id==='claude-haiku-5-5')&&data.data.some(x=>x.id==='gpt-6.2-sol'),'model catalog regression');actual={models:data.data.length};
    }else if(scenario.family==='count'||scenario.family==='unsupported'){
      delete body.max_tokens;delete body.system;
      if(scenario.family==='unsupported')body.model='gpt-6.2-sol';
      try{const value=await messages.countTokens(body);check(scenario.family==='count','Codex count unexpectedly succeeded');equal(value,{input_tokens:37},'count response');actual=value;}
      catch(e){check(scenario.family==='unsupported'&&e.status===404&&e.data?.type==='error','unexpected count error '+e.message);actual={status:e.status,error:e.data};}
    }else if(scenario.family==='context'){
      context.resetContext();
      const contextValue={edits:[{type:'clear_tool_uses_20250919',keep:{type:'tool_uses',value:3}}]};
      const send=()=>messages.create({...body,betas:context.remainingBetas(),...context.contextBody(contextValue)});
      try{await send();throw Error('context rejection missing');}catch(e){
        check(e.status===400,'context error status');
        const reason=context.heal(e);
        equal(Boolean(reason),scenario.reject,'original context rejection branch');
        if(reason){const value=await send();check(value.content[0].text==='P3 JSON','context retry response');}
        actual={reason:reason??null};
      }
    }else if(scenario.family==='retry'){
      try{const value=await messages.create(body);check(scenario.allow&&value.content[0].text==='P3 JSON','retry response');actual={terminal:value};}
      catch(e){check(!scenario.allow&&e.status===429&&e.data.error.message==='P3 limited','retry deny failed '+e.message);actual={status:e.status,error:e.data};}
      equal(waits.slice(startWaits),scenario.allow?[scenario.ms]:[],'retry-after-ms precedence');
    }else if(scenario.family==='sse'){
      let delta=false;
      const stream=ff.createMessage(messages,body);
      stream.on('streamEvent',e=>{if(e.type==='content_block_delta'&&e.delta.type==='text_delta')delta=true;});
      const final=stream.finalMessage();final.catch(()=>{});
      const deadline=Date.now()+5000;
      while(!(await control('/state')).fragment_sent){check(Date.now()<deadline,'fragment deadline');await new Promise(r=>setTimeout(r,10));}
      while(!delta&&Date.now()<deadline)await new Promise(r=>setTimeout(r,10));
      check((await control('/state')).closed,'terminal barrier opened early');check(delta,'no decoded text before terminal release');
      await control('/release');const value=await final;
      equal(value.content,[{type:'text',text:'P3 terminal '+scenario.id}],'terminal text');
      equal(value.usage,{input_tokens:17,output_tokens:11},'terminal usage');check(value.stop_reason==='end_turn','terminal reason');actual={beforeRelease:delta,message:value};
    }else{
      const value=await messages.create(body);check(value.content[0].text==='P3 JSON','JSON Messages regression');actual=value;
    }
  }catch(e){failure=e.stack;}
  const fixture=await control('/finish');
  try{
    check(!fixture.failures.length,'fixture failed');
    const expectedRequests=['catalog','unsupported'].includes(scenario.family)?0:
      (scenario.family==='retry'&&scenario.allow||scenario.family==='context'&&scenario.reject)?2:1;
    equal(fixture.receipts.length,expectedRequests,'upstream HTTP attempt count');
    for(let index=0;index<fixture.receipts.length;index++){
      const receipt=fixture.receipts[index],wire=wires[startWires+(scenario.family==='context'?index:0)];
      check(wire,'missing SDK wire');
      const expected={...wire.body,model:'claude-haiku-5-5'};
      structure(receipt.body,expected,'complete upstream JSON fidelity');
      equal(receipt.path,wire.path,'query/path preservation');
      const headers=Object.fromEntries(Object.entries(receipt.headers).map(([k,v])=>[k.toLowerCase(),v]));
      equal(headers['anthropic-version'],wire.headers['anthropic-version'],'version pairing');
      const betas=headers['anthropic-beta'].split(',');
      for(const feature of wire.headers['anthropic-beta'].split(','))check(betas.includes(feature),'beta/field pairing lost');
      if(scenario.family==='context'&&index===1)check(!betas.includes(beta),'rejected context beta reinserted');
      if(scenario.auth==='oauth')check(headers.authorization==='Bearer sk-ant-oat01-p3-synthetic'&&!headers['x-api-key'],'OAuth auth path');
      else check(headers['x-api-key']==='p3-synthetic-api-key','API-key auth path');
    }
    for(const response of responses.slice(startResponses)){
      equal(response.headers['request-id'],fixture.receipts.length?'req_p3':undefined,'request ID forwarding');
      check(!response.headers['x-private-upstream']&&!response.headers['anthropic-ratelimit-unified-hidden'],'excluded response header leaked');
      if(response.status===429){equal(response.headers['retry-after-ms'],String(scenario.ms),'millisecond header');equal(response.headers['retry-after'],String(scenario.seconds),'seconds header');equal(response.headers['x-should-retry'],String(scenario.allow),'retry decision header');}
      if(response.status===429)equal(response.rawBody,'{"type":"error","error":{"type":"rate_limit_error","message":"P3 limited"}}','error response bytes');
      if(scenario.family==='count')equal(response.rawBody,'{"input_tokens":37}','count response bytes');
      if(response.status!==404)equal(response.headers['anthropic-ratelimit-unified-status'],'allowed','rate limit header');
    }
  }catch(e){failure=[failure,e.stack].filter(Boolean).join('\n');}
  const layers={fidelity:'original SXn -> zi.create -> makeRequest; complete JSON comparison',
    retry:'original makeRequest -> shouldRetry -> retryRequest -> makeRequest; controlled Ut and HTTP transport',
    context:'original vSt/g1/T1n/tHe -> lUo removal closure -> original request-field gate -> zi.create; controlled registry/hooks',
    count:'original zi.countTokens -> makeRequest -> JSON response',unsupported:'original zi.countTokens -> gateway 404; main p8t fallback static only',
    json:'original zi.create -> makeRequest -> JSON response',sse:'original zi.create -> makeRequest -> zt/cc -> ff.finalMessage',
    catalog:'bounded fetch to local catalog; original CLI discovery cache not executed'};
  outcomes.push({id:scenario.id,family:scenario.family,expected:scenario.expected,layer:layers[scenario.family],
    gateway_path:scenario.family==='catalog'?'GET /v1/models?limit=1000':
      ['count','unsupported'].includes(scenario.family)?'POST /v1/messages/count_tokens?beta=true':'POST /v1/messages?beta=true',actual,fixture,
    waits:waits.slice(startWaits),responses:responses.slice(startResponses),result:failure?1:0,error:failure});
}
console.log(JSON.stringify({outcomes,wires,redirectRejected,result:outcomes.some(x=>x.result)?1:0}));
process.exitCode=outcomes.some(x=>x.result)?1:0;
