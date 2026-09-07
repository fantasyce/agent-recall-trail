const params = new URLSearchParams(location.search);
const session = params.get("session");
let state;
const $ = (selector) => document.querySelector(selector);
const status = (message) => {
  const node = $("#status");
  node.textContent = message;
  node.classList.add("visible");
  window.setTimeout(() => node.classList.remove("visible"), 2800);
};
async function request(path, options = {}) {
  const response = await fetch(path, options);
  const payload = await response.json().catch(() => ({}));
  if (!response.ok) throw new Error(payload.error || "请求失败 (" + response.status + ")");
  return payload;
}
function setView(view) {
  document.querySelectorAll(".view").forEach((node) => { node.hidden = node.id !== view + "-view"; });
  document.querySelectorAll("[data-view]").forEach((node) => {
    if (node.dataset.view === view) node.setAttribute("aria-current", "page");
    else node.removeAttribute("aria-current");
  });
}
function renderProposals() {
  const list = $("#proposal-list");
  const proposals = state.proposals.filter((item) => ["submitted", "under_review", "approved"].includes(item.status));
  if (!proposals.length) {
    list.innerHTML = '<p class="empty">目前没有等待处理的知识提案。</p>';
    return;
  }
  list.replaceChildren(...proposals.map((proposal) => {
    const card = document.createElement("article");
    card.className = "proposal";
    const copy = document.createElement("div");
    const title = document.createElement("h3");
    title.textContent = proposal.title;
    const applicability = document.createElement("p");
    applicability.textContent = proposal.applicability;
    const meta = document.createElement("p");
    meta.className = "proposal-meta";
    meta.textContent = proposal.proposal_id + "@" + proposal.revision + " · " + proposal.status + " · " + proposal.risk;
    copy.append(title, applicability, meta);
    const actions = document.createElement("div");
    actions.className = "actions";
    if (["submitted", "under_review"].includes(proposal.status)) {
      const approve = document.createElement("button");
      approve.type = "button";
      approve.className = "action primary";
      approve.textContent = "批准";
      approve.addEventListener("click", () => reviewProposal(proposal, "approved"));
      const changes = document.createElement("button");
      changes.type = "button";
      changes.className = "action";
      changes.textContent = "要求修改";
      changes.addEventListener("click", () => reviewProposal(proposal, "changes_requested"));
      actions.append(approve, changes);
    }
    if (proposal.status === "approved") {
      const publish = document.createElement("button");
      publish.type = "button";
      publish.className = "action primary";
      publish.textContent = "发布";
      publish.addEventListener("click", () => publishProposal(proposal));
      actions.append(publish);
    }
    card.append(copy, actions);
    return card;
  }));
}
function renderAudit() {
  const list = $("#audit-list");
  if (!state.audit.length) {
    list.innerHTML = '<li class="empty">尚无 Agent 委托治理记录。</li>';
    return;
  }
  list.replaceChildren(...state.audit.map((entry) => {
    const item = document.createElement("li");
    const time = document.createElement("time");
    time.textContent = new Date(entry.created_at).toLocaleString();
    const line = document.createElement("p");
    line.textContent = (entry.operation === "approve" ? "批准" : "发布") + " · " + entry.actor_type;
    const detail = document.createElement("small");
    detail.textContent = entry.proposal_id + "@" + entry.revision + " · " + entry.agent_id + " · host " + entry.host_binding;
    item.append(time, line, detail);
    return item;
  }));
}
async function refresh() {
  state = await request("/api/bootstrap?session=" + encodeURIComponent(session || ""));
  $("#connection-dot").classList.add("ready");
  $("#agent-label").textContent = state.bound_agent_id + " · host " + state.host_binding;
  $("#session-expiry").textContent = "会话有效至 " + new Date(state.expires_at).toLocaleTimeString();
  const toggle = $("#delegation-toggle");
  toggle.checked = state.governance_mode === "delegated_local";
  $("#policy-explanation").textContent = toggle.checked
    ? "已开启。明确的当前用户指令可由此 Agent 一次完成批准与发布。"
    : "已关闭。提案必须在此页面由人类审核和发布。";
  renderProposals();
  renderAudit();
  setView(state.view || "pending");
}
async function mutate(path, body) {
  return request(path, {
    method: "POST",
    headers: { "content-type": "application/json", "x-art-csrf": state.csrf_token },
    body: JSON.stringify({ session, ...body }),
  });
}
async function reviewProposal(proposal, decision) {
  const reason = window.prompt("填写审核理由（必填）");
  if (!reason || !reason.trim()) return;
  try {
    await mutate("/api/review", { proposal_id: proposal.proposal_id, revision: proposal.revision, decision, reason: reason.trim() });
    status(decision === "approved" ? "已批准" : "已要求修改");
    await refresh();
  } catch (error) { status(error.message); }
}
async function publishProposal(proposal) {
  if (!window.confirm("发布后会生成不可变的共享 Knowledge Edition。继续吗？")) return;
  try {
    await mutate("/api/publish", { proposal_id: proposal.proposal_id, revision: proposal.revision, confirm: true });
    status("已发布");
    await refresh();
  } catch (error) { status(error.message); }
}
document.querySelectorAll("[data-view]").forEach((button) => {
  button.addEventListener("click", () => setView(button.dataset.view));
});
$("#delegation-toggle").addEventListener("change", async (event) => {
  const mode = event.target.checked ? "delegated_local" : "human_review";
  event.target.disabled = true;
  try {
    await mutate("/api/delegation", { mode });
    status(mode === "delegated_local" ? "Agent 委托治理已开启" : "Agent 委托治理已关闭");
    await refresh();
  } catch (error) {
    event.target.checked = !event.target.checked;
    status(error.message);
  } finally {
    event.target.disabled = false;
  }
});
refresh().catch((error) => {
  $("#agent-label").textContent = "本地会话不可用";
  status(error.message);
});
