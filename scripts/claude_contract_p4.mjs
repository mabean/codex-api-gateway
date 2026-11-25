// Bounded adapters around fingerprinted original client code. Never runs the CLI.
import fs from 'node:fs';
import vm from 'node:vm';
const [directory,base,upstream,casesPath]=process.argv.slice(2);
const cases=JSON.parse(fs.readFileSync(casesPath)).cases;
const ranges=JSON.parse(fs.readFileSync(directory+'/p4-ranges.json'));
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
vm.runInContext('class Retry {'+ranges.makeRequest+ranges.retry+'}\n'+ranges.turn+'\nglobalThis.p4={Retry,SXn};',context,{timeout:2000});
const {zi,zt,ff}=context.sdk;
const {Retry,SXn}=context.p4;
class APIError extends Error{constructor(status,message){super(message);this.status=status;}}
Object.assign(context,{Nt:APIError,dHe:()=>null,WN:()=>false,g1:()=>false,a5:{},FBt:e=>e.message,ysr:()=>false,kq:{header:'per-turn-control'}});
vm.runInContext(ranges.scopeRegex+ranges.Ime+ranges.l1n+';globalThis.refusalPredicates={Ime,l1n};',context);
vm.runInContext(ranges.contextBuilder+';globalThis.contextBuilder=_Bn;',context);
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
    headers.set('x-api-key','p4-synthetic-api-key');
    wires.push({id:current.id,path,body:options.body,headers:Object.fromEntries(headers)});
    const raw=retry.makeRequest({...options,path,headers,maxRetries:2}).then(value=>({...value,
      data:options.body.stream?zt.fromSSEResponse(value.response,new AbortController()):value.response.json()}));
    const promise=raw.then(async value=>await value.data);
    promise.catch(()=>{});
    promise.withResponse=()=>raw.then(async value=>({...value,data:await value.data}));
    return promise;
  }};
}

// Only capability/fixture adapters are synthetic. Every selected native branch
// is loaded as its original byte span; dormant import.meta.require fails closed.
const identity=x=>x, noop=()=>{}, off=()=>false;
Object.assign(context,{En:undefined,fo:'ToolSearch',q_:()=>({}),o$e:()=>({}),x_e:()=>new Set(['ToolSearch']),
  Tus:off,ue:off,pg:off,Lzi:off,pI:()=>[],b:()=>0,xds:'fixture',ixn:'fixture-cache',c:noop,C:noop,qt:noop,nI:noop,ZJ:noop,
  xte:identity,lci:identity,bme:identity,E2e:identity,u:identity,ve:identity,
  Qt:(tools,name)=>tools.find(t=>t.name===name),M:xs=>[...new Set(xs)],wir:/^select:(.+)$/i,
  Mt:(s,p)=>s.split(p).length-1,B:(xs,p)=>xs.filter(p).length,nh:t=>t.name,a:noop,t:noop,
  va:'unused-va',ht:'unused-ht',G:'unused-G',NF:'unused-NF',N:off,kE:off,
  H:noop,SH:noop,yUt:identity,QBe:Error,R:()=>['gpt'],aZ:off,_:JSON.stringify,
  Jr:()=>({once:off}),f:noop,m:noop,i:e=>{throw e;}});
async function native(name,source,exports){
  const module=new vm.SourceTextModule(source+'\n'+exports,{context,identifier:'p4-original-'+name,
    initializeImportMeta(meta){meta.require=()=>{throw Error('unavailable native capability import');};}});
  await module.link(()=>{throw Error('unavailable native import');});await module.evaluate({timeout:2000});
}
await native('wrapper',ranges.wrapper,'globalThis.En=En;');
await native('eligibility',ranges.eligibility,'globalThis.eL=eL;');
await native('history',ranges.history,'globalThis.history={lwi,$$e};');
await native('support',ranges.support,'globalThis.cUt=cUt;');
await native('fallback',ranges.fallback,'globalThis.gyt=gyt;');
await native('validation',ranges.validation,'globalThis.validation={Gnr,j5o};');
await native('ToolSearch',ranges.h1+ranges.search+ranges.vGn+ranges.l4o,'globalThis.searchTool=l4o;');
check(context.cUt('gpt-6.2-sol')===false,'original support registry branch unavailable');
const schema={type:'object',properties:{file_path:{type:'string'}},required:['file_path']};
let executions=0,permission='allow';
const read=context.En({name:'Read',shouldDefer:true,inputJSONSchema:schema,
  inputSchema:{safeParse(input){return input?.file_path==='/fixture/p4.txt'?{success:true,data:input}:{success:false,error:'fixture path required'};}},
  create(){return {validateInput:async input=>({result:input.file_path==='/fixture/p4.txt'}),
    checkPermissions:async input=>({behavior:permission,updatedInput:input}),
    async call(input){executions++;return {data:'P4 fixture content\n'+input.file_path};}};}});
const fixture={tools:[context.searchTool,read],baseTools:()=>[],turnMcpClients:[],
  toolState:{get:()=>({maybeInvalidate:noop})},messages:()=>[],permissions:()=>({}),
  contextRoom:noop,modelFacts:()=>({}),abortController:new AbortController()};
check(context.eL(read,new Set(['ToolSearch']),fixture.tools,{nonDeferrableBuiltins:[]})===true,'native eligibility declined deferred Read');
check(context.eL(context.searchTool,new Set(['ToolSearch']),fixture.tools,{nonDeferrableBuiltins:[]})===false,'native ToolSearch deferred itself');
async function nativeRead(input,allowed){
  permission=allowed?'allow':'deny';
  const parsed=context.validation.Gnr(read,input);
  check(parsed.ok,'native schema validation failed');
  const valid=await context.validation.j5o(read,parsed.input,value=>read.validateInput(value,fixture));
  check(valid.ok,'native input validation failed');
  const verdict=await read.checkPermissions(valid.callInput,fixture);
  const before=executions;
  if(verdict.behavior!=='allow'){check(executions===before,'denied fixture ran');return {denied:true,executions:0};}
  const result=await read.call(verdict.updatedInput,fixture);
  check(executions===before+1,'fixture executor count');return result;
}
const wiresAt=()=>wires.length;
async function send(body){const stream=ff.createMessage(new zi(sdkClient()),body);return stream.finalMessage();}
const textOf=message=>message.content.filter(b=>b.type==='text').map(b=>b.text).join('');
const outcomes=[];
for(const scenario of cases){
  current=scenario;await control('/control',scenario);
  let failure,actual={};
  try{
    let body={model:'gpt-6.2-sol',max_tokens:64,system:[{type:'text',text:'# System\nP4 base'},{type:'text',text:'P4 ordered'}],
      messages:[{role:'user',content:scenario.id}],betas:['per-turn-control-2026-07-01','advanced-tool-use-2025-11-20']};
    const id=scenario.id;
    let reject=false,expectedEffort;
    if(id.startsWith('effort_')){
      const value=id.slice(7);
      if(value!=='absent')body.output_config={effort:['override','only','followup'].includes(value)?'low':value};
      expectedEffort=value==='absent'?undefined:['override','only','followup'].includes(value)?'xhigh':value;
      if(['override','only','followup'].includes(value)){
        const controls={dropBase:new Set(),clearAtAfter:new Map(),statementAfter:new Map([[0,{effort:'medium'}],[1,{effort:'xhigh'}]])};
        body.messages.push({role:'assistant',content:[{type:'text',text:'previous'}]});
        body.messages=SXn(body.messages,controls).wire;
        if(value==='only')delete body.system;
        if(value==='followup')body.messages.push({role:'user',content:'P4 subsequent user'});
      }
    }
    if(['scope_first','scope_followup','mid_history'].includes(id)){
      body.messages.push({role:'assistant',content:[{type:'text',text:'previous'}]});
      body.messages=SXn(body.messages,{dropBase:new Set(),clearAtAfter:new Map([[1,[{type:'text',text:'P4 scoped'}]]]),statementAfter:new Map()}).wire;
      if(id==='mid_history')delete body.messages[2].clear_at;
      if(id==='scope_followup')body.messages.push({role:'user',content:'followup'});
      reject=id!=='mid_history';
    }
    if(['bad_effort','max_effort'].includes(id)){body.output_config={effort:id==='max_effort'?'max':7};reject=true;}
    if(['format','format_context'].includes(id)){body.output_config={format:{type:'json_schema',schema:{type:'object'}}};reject=true;}
    if(['context','format_context'].includes(id)){body.context_management={edits:[{type:'clear_tool_uses_20250919'}]};reject=true;}
    if(id==='unknown_output'){body.output_config={mystery:true};reject=true;}
    if(id.startsWith('choice_')){
      body.tools=[{name:'Read',input_schema:schema}];
      const kind=id.slice(7);
      body.tool_choice=kind==='malformed'?'auto':{type:kind==='named'||kind==='unavailable'?'tool':kind.startsWith('parallel')?'auto':kind};
      if(kind==='named')body.tool_choice.name='Read';
      if(kind==='unavailable')body.tool_choice.name='NotebookEdit';
      if(kind==='parallel')body.tool_choice.disable_parallel_tool_use=true;
      if(kind==='parallel_false')body.tool_choice.disable_parallel_tool_use=false;
      reject=['bad','unavailable','malformed'].includes(kind);
    }
    if(['discovery_chain','unsupported_reference','undeclared_reference','unrelated_reference','legacy_read','standard_read'].includes(id)){
      const legacy=['legacy_read','standard_read'].includes(id);
      const standard=id==='standard_read';
      const standardBlocks=[{type:'text',text:'# Tools\nstandard context\n',cache_control:{type:'ephemeral'}},
        {type:'text',text:'[gateway-model: claude-fable-5-2]\nordered context'}];
      if(standard)body.messages.push({role:'system',content:standardBlocks});
      const assertStandard=translated=>{
        equal(translated.model,'gpt-6.2-sol','system directive cannot reroute');
        structure(translated.input.slice(0,2),[{role:'user',content:[{type:'input_text',text:id}]},
          {role:'system',content:standardBlocks.map(b=>({type:'input_text',text:b.text}))}],'ordinary ordered system blocks');
        check(!translated.instructions.includes('standard context'),'inline context was hoisted');
      };
      if(legacy){
        body.context_management=context.contextBuilder({hasThinking:true});
        structure(body.context_management,{edits:[{type:'clear_thinking_20251015',keep:'all'}]},'original retain-all builder');
        body.output_config={effort:'low'};
      }
      body.tools=[{name:'ToolSearch',input_schema:{type:'object',properties:{query:{type:'string'}},required:['query']}},
        {name:'Read',description:'Synthetic fixture Read',input_schema:schema,...!legacy&&{defer_loading:true}}];
      if(legacy)body.tools.shift();
      const first=await (async()=>{
        await control('/control',{...scenario,stage:legacy?'read':'search'});
        const message=await send(body);const receipt=await control('/finish');
        check(receipt.receipts.length===1,'initial native chain upstream count');
        equal(receipt.receipts[0].body.tools.map(t=>t.name),legacy?['Read']:['ToolSearch'],'initial deferred exposure');
        if(standard)assertStandard(receipt.receipts[0].body);
        if(legacy){
          structure(receipt.receipts[0].body.reasoning,{effort:'low'},'retain-all first-turn effort');
          structure(wires.at(-1).body.context_management,body.context_management,'retain-all first-turn SDK serialization');
        }
        return message;
      })();
      const use=first.content.find(b=>b.type==='tool_use');check(use?.name===(legacy?'Read':'ToolSearch'),'extracted SDK tool call');
      body.messages.push({role:'assistant',content:first.content});
      if(!legacy){
        const result=await context.searchTool.create(fixture).call(use.input,{signal:fixture.abortController.signal,toolUseId:use.id});
        equal(result.data.matches,['Read'],'native ToolSearch selection');
        const mapped=context.searchTool.mapToolResultToToolResultBlockParam(result.data,use.id);
        equal(context.history.lwi([mapped]),[{toolUseId:use.id,toolName:'Read'}],'native reference history');
        const fallback=context.gyt({...body,messages:[{role:'user',content:[mapped]}]});
        check(fallback.messages[0].content[0].content.some(b=>b.text?.includes('"parameters"')),'original fallback lacks schema');
        if(id==='unsupported_reference'){mapped.content[0].tool_name='NotebookEdit';body.tools.push({name:'NotebookEdit',input_schema:schema,defer_loading:true});reject=true;}
        if(id==='undeclared_reference'){mapped.content[0].tool_name='Write';reject=true;}
        if(id==='unrelated_reference'){mapped.tool_use_id='unrelated';reject=true;}
        mapped.content.unshift({type:'text',text:'before reference'});mapped.content.push({type:'text',text:'after reference'});
        body.messages.push({role:'user',content:[mapped]});
        await control('/control',{...scenario,stage:'read'});
        if(!reject){
          const second=await send(body);const receipt=await control('/finish');
          const translated=receipt.receipts[0].body;
          structure(translated.tools.find(t=>t.name==='Read').parameters,schema,'activated exact Read schema');
          const output=translated.input.find(i=>i.type==='function_call_output');
          check(output.call_id===use.id&&output.output.indexOf('before reference')<output.output.indexOf('"parameters"')&&output.output.indexOf('after reference')>output.output.indexOf('"parameters"'),'schema-bearing output order/correlation');
          const readUse=second.content.find(b=>b.type==='tool_use');
          check(readUse?.name==='Read','discovered native Read call missing');
          body.messages.push({role:'assistant',content:second.content});
        }
      }
      if(!reject){
        const readUse=body.messages.at(-1).content.find(b=>b.type==='tool_use');
        const before=executions;
        actual.denied=await nativeRead(readUse.input,false);check(executions===before,'denied Read executed');
        check(context.validation.Gnr(read,{file_path:'/wrong'}).ok===false,'invalid fixture input accepted');
        const result=await nativeRead(readUse.input,true);check(executions===before+1,'Read did not execute exactly once');
        equal(result.data,'P4 fixture content\n/fixture/p4.txt','fixture contents');
        body.messages.push({role:'user',content:[{type:'tool_result',tool_use_id:readUse.id,content:result.data}]});
        if(standard)body.messages.push({role:'system',content:'standard followup context'});
        await control('/control',scenario);
        actual.execution={count:executions-before,call_id:readUse.id,content:result.data};
      }
    }
    const responsesBefore=responses.length,wiresBefore=wiresAt();
    if(reject){
      try{await new zi(sdkClient()).create({...body,stream:false});throw Error('required terminal refusal disappeared');}
      catch(error){
        check(error instanceof APIError&&error.status===400&&error.data?.type==='error'&&error.data.error.type==='invalid_request_error','structured SDK refusal missing '+error.message);
        check(context.refusalPredicates.Ime(error)===false&&context.refusalPredicates.l1n(error)===false,'native client would silently downgrade refusal');
        actual.error={status:error.status,body:error.data,effortFallback:false,scopeFallback:false};
      }
      check(responses.length-responsesBefore===1&&wiresAt()-wiresBefore===1,'refusal retried');
    }else{
      const terminal=await send(body);equal(textOf(terminal),'P4 terminal '+id,'terminal text');
      equal(terminal.stop_reason,'end_turn','terminal stop reason');actual.terminal=terminal;
    }
    const receipt=await control('/finish');
    check(receipt.failures.length===0,'fixture failure');
    equal(receipt.receipts.length,reject?0:1,'terminal upstream count');
    if(!reject){
      const translated=receipt.receipts[0].body;
      if(id.startsWith('effort_'))structure(translated.reasoning??null,expectedEffort?{effort:expectedEffort}:null,'exact effective effort');
      if(id!=='effort_only')check(translated.instructions.indexOf('P4 base')<translated.instructions.indexOf('P4 ordered'),'top system order');
      if(id.startsWith('choice_')){
        const kind=id.slice(7);structure(translated.tool_choice,kind==='named'?{type:'function',name:'Read'}:kind==='any'?'required':kind.startsWith('parallel')?'auto':kind,'tool selection');
        equal(translated.parallel_tool_calls,kind!=='parallel','parallel inversion');
      }
      if(actual.execution){const output=translated.input.filter(i=>i.type==='function_call_output').at(-1);equal(output.call_id,actual.execution.call_id,'Read result correlation');equal(output.output,actual.execution.content,'Read result content');}
      if(id==='mid_history'){
        structure(translated.input,[{role:'user',content:[{type:'input_text',text:id}]},
          {role:'system',content:[{type:'input_text',text:'P4 scoped'}]}],'original non-temporary SXn position');
      }
      if(id==='standard_read'){
        structure(translated.input.slice(0,2),[{role:'user',content:[{type:'input_text',text:id}]},
          {role:'system',content:[{type:'input_text',text:'# Tools\nstandard context\n'},
            {type:'input_text',text:'[gateway-model: claude-fable-5-2]\nordered context'}]}],'followup keeps original system position');
        const items=translated.input;
        check(items[2].type==='function_call'&&items[2].call_id===actual.execution.call_id,'standard tool call correlation');
        check(items[3].type==='function_call_output'&&items[3].call_id===actual.execution.call_id,'standard result adjacency');
        structure(items[4],{role:'system',content:[{type:'input_text',text:'standard followup context'}]},'followup ordered context');
        equal(items.length,5,'standard history length');
      }
      if(['legacy_read','standard_read'].includes(id)){
        structure(translated.reasoning,{effort:'low'},'retain-all followup effort');
        structure(wires.at(-1).body.context_management,body.context_management,'retain-all followup SDK serialization');
        actual.contextBuilder={name:'_Bn',value:body.context_management,turns:2};
      }
    }
    actual.fixture=receipt;
  }catch(error){failure=error.stack??String(error);}
  outcomes.push({id:scenario.id,expected:scenario.expected,actual,result:failure?1:0,...failure&&{error:failure}});
}
console.log(JSON.stringify({outcomes,wires,responses,executions,result:outcomes.some(o=>o.result)?1:0}));
process.exitCode=outcomes.some(o=>o.result)?1:0;
