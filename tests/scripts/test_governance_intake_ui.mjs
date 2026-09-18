import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';

// A small DOM port: execute the shipped renderer and dispatch its real events.
class Element {
  children = []; dataset = {}; listeners = {}; value = ''; textContent = ''; disabled = false;
  classList = { add() {}, remove() {} };
  constructor(tag = 'div') { this.tag = tag; }
  append(...nodes) { this.children.push(...nodes); }
  replaceChildren(...nodes) { this.children = nodes; }
  setAttribute() {} removeAttribute() {} focus() {}
  addEventListener(type, fn) { this.listeners[type] = fn; }
  querySelectorAll(tag) { return this.children.flatMap(n => [ ...(n.tag === tag ? [n] : []), ...n.querySelectorAll(tag)]); }
  reportValidity() { return true; }
  get text() { return this.textContent + this.children.map(n => n.text).join(' '); }
}
const nodes = new Map();
const document = {querySelector(selector) { if (!nodes.has(selector)) nodes.set(selector,new Element()); return nodes.get(selector); }, querySelectorAll(){return [];},createElement(tag){return new Element(tag);}};
const context = vm.createContext({document,location:{search:'?session=capability'},URLSearchParams,window:{setInterval(){},setTimeout(){}},fetch(){return new Promise(()=>{});},Date,console});
vm.runInContext(fs.readFileSync(new URL('../../crates/art-mcp/assets/governance/app.js',import.meta.url),'utf8'),context);
vm.runInContext(`ui.bootstrap={auto_memory:{settings:{enabled:true,max_captures_per_session:3,cooldown_seconds:600},diagnostics:{origins:{agent_initiated:{last_disposition:'pending_review'},hook_triggered:{last_disposition:'activated'}},shared_budget:[{bucket:'agent-day:2026-09-17',used:2,degraded:true}],pending_count:2,pending_revision_count:1}}}; renderAutoMemory();`,context);
assert.match(nodes.get('#auto-memory-explanation').text,/Agent.*Hook/);
const diagnosticText=nodes.get('#auto-memory-diagnostics').text;
assert.match(diagnosticText,/pending_review/); assert.match(diagnosticText,/activated/);
assert.match(diagnosticText,/2\/3/); assert.match(diagnosticText,/降级/); assert.match(diagnosticText,/DSH/);
const proposal={proposal_id:'proposal-1',target_memory_id:'memory-1',expected_revision:4,artifact:{id:'incoming',current_revision:1,title:'Proposed title',summary:'Proposed summary',payload:{kind:'semantic',data:{statement:'Proposed content',applicability:'repo',exceptions:[]}}},current_artifact:{id:'memory-1',current_revision:4,title:'Active title',summary:'Active summary',payload:{kind:'semantic',data:{statement:'Active content'}}},anchors:[{kind:'user_statement',locator:'user:correction',observed_at:'2026-09-17',metadata:{scope:'repository'}}],receipt:{origin:'agent_initiated',value_reason:'Prevents repeated repair',reason:'revision_requires_human_review',policy_version:'v1'}};
context.proposal=proposal;
vm.runInContext(`renderMemoryCandidates([], [proposal]);`,context);
const list=nodes.get('#memory-candidate-list');
assert.match(list.text,/Active content/); assert.match(list.text,/Prevents repeated repair/); assert.match(list.text,/2026-09-17/);
assert.equal(nodes.get('#candidate-count').textContent,'1');
context.sent=[];
vm.runInContext(`mutate=async(path,body)=>{sent.push({path,body});return {};};refresh=async()=>{};`,context);
for (const kind of ['proposal', 'candidate']) {
  for (const [action, buttonText] of [['confirm', '确认并启用'], ['edit_confirm', '修改后确认'], ['reject', '拒绝']]) {
    vm.runInContext(kind === 'proposal' ? `renderMemoryCandidates([], [proposal]);` : `renderMemoryCandidates([{...proposal,proposal_id:null,target_memory_id:null,expected_revision:null}]);`,context);
    const form=list.querySelectorAll('form')[0];
    assert.ok(form);
    const field = (label) => {
      const index = form.children.findIndex(node => node.tag === 'label' && node.textContent === label);
      assert.notEqual(index,-1,`Missing ${label} field`);
      return form.children[index+1];
    };
    field('标题').value = 'Human edited title';
    field('摘要').value = 'Human edited summary';
    field('记忆内容').value = 'Human edited statement';
    field('适用范围').value = 'Future repository maintenance';
    field('例外（每行一项）').value = 'Changed evidence\nChanged scope';
    field('决定理由').value = `Inspected sources for ${kind} ${action}`;
    const button = form.querySelectorAll('button').find(node => node.textContent === buttonText);
    assert.ok(button,`Missing ${buttonText} button`);
    button.listeners.click?.({target:button});
    await form.listeners.submit({preventDefault(){},submitter:button});
    const sent = context.sent.at(-1);
    assert.equal(sent.path,kind === 'proposal' ? '/api/memory-revision-review' : '/api/memory-candidate-review');
    assert.equal(sent.body.action,action);
    assert.equal(sent.body.reason,`Inspected sources for ${kind} ${action}`);
    if (kind === 'proposal') {
      assert.equal(sent.body.proposal_id,'proposal-1');
      assert.equal(sent.body.expected_revision,4);
      assert.equal(sent.body.memory_id,undefined);
    } else {
      assert.equal(sent.body.memory_id,'incoming');
      assert.equal(sent.body.revision,1);
      assert.equal(sent.body.proposal_id,undefined);
    }
    if (action === 'edit_confirm') {
      assert.equal(sent.body.title,'Human edited title');
      assert.equal(sent.body.summary,'Human edited summary');
      assert.deepEqual(JSON.parse(JSON.stringify(sent.body.payload)),{kind:'semantic',data:{statement:'Human edited statement',applicability:'Future repository maintenance',exceptions:['Changed evidence','Changed scope']}});
    } else {
      assert.equal(sent.body.title,undefined);
      assert.equal(sent.body.summary,undefined);
      assert.equal(sent.body.payload,undefined);
    }
  }
}
assert.equal(context.sent.length,6);
vm.runInContext(`ui.bootstrap.audit=[];ui.bootstrap.memory_reviews=[{proposal_id:'proposal-1',memory_id:'memory-1',expected_revision:4,revision:5,action:'confirm',actor:'human:local-governance-ui',reason:'Inspected evidence',reviewed_at:'2026-09-17'}];renderAudit();`,context);
assert.match(document.querySelector('#memory-review-audit').text,/memory-1@4.*5/);
assert.match(document.querySelector('#memory-review-audit').text,/Inspected evidence/);
console.log('governance intake UI behavior: passed');
