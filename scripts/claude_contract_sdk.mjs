// The contract logic is loaded verbatim from the pinned binary, never vendored.
import fs from 'node:fs';
import vm from 'node:vm';
const [source, base, fixturePath] = process.argv.slice(2);
const fixture = JSON.parse(fs.readFileSync(fixturePath, 'utf8'));
class SDKError extends Error {}
class AbortError extends SDKError {}
const unsupported = class { constructor() { throw new Error('unresolved SDK dependency invoked'); } };
const context = vm.createContext({console, Headers, TextEncoder, TextDecoder, ReadableStream,
  AbortController, Buffer, setTimeout, clearTimeout,
  Ao: SDKError, Jc: AbortError, Zdt: e => e?.name === 'AbortError',
  ni: 'x-stainless-helper-method', nd: unsupported, rb: unsupported});
vm.runInContext(fs.readFileSync(source, 'utf8')+'\n globalThis.contract = {zi, zt, ff};', context,
  {filename: 'pinned-claude-sdk.js', timeout: 2000});
const {zi, zt, ff} = context.contract;
const requests = [];
const client = {
  _options: {timeout: 5000},
  post(path, options) {
    const url = new URL(path, base);
    if (url.hostname !== '127.0.0.1' || url.origin !== base) throw Error('transport rejects non-loopback destination');
    const headers = new Headers(options.headers?.values);
    headers.set('content-type', 'application/json');
    headers.set('x-api-key', 'p1-synthetic-client');
    headers.set('anthropic-version', '2023-06-01');
    requests.push({method:'POST', path:url.pathname+url.search, body:options.body});
    const promise = fetch(url, {method:'POST', headers, body:JSON.stringify(options.body),
      redirect:'error', signal:AbortSignal.any([options.signal, AbortSignal.timeout(5000)].filter(Boolean))})
      .then(response => {
        if (!response.ok) throw Error('gateway status '+response.status);
        return {response, data:zt.fromSSEResponse(response, new AbortController())};
      });
    return {withResponse: () => promise};
  }
};
const messages = new zi(client);
const outcomes = [];
for (const scenario of fixture.scenarios) {
  const stream = ff.createMessage(messages, {...fixture.request,
    messages:[{role:'user',content:scenario.prompt}]});
  try {
    const text = await stream.finalText();
    if (scenario.error || text !== scenario.text) throw Error('unexpected terminal text: '+JSON.stringify(text));
    outcomes.push({id:scenario.id, text, parser:'zt.fromSSEResponse -> cc', branch:'ff.finalText', result:0});
  } catch (error) {
    if (!scenario.error || !error.message.includes(scenario.error)) throw error;
    outcomes.push({id:scenario.id, error:error.message, parser:'zt.fromSSEResponse -> cc',
      branch:'ff accumulator uf: before message_start', result:0});
  }
}
console.log(JSON.stringify({requests, outcomes}));
