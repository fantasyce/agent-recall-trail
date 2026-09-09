const params = new URLSearchParams(location.search);
const session = params.get("session");
const $ = (selector) => document.querySelector(selector);
const dialog = $("#proposal-dialog");
const ui = {
  bootstrap: null,
  detail: null,
  trigger: null,
  decision: null,
  busy: false,
  expired: false,
  conflict: false,
  autoOpened: false,
  warned: false,
};

const labels = {
  submitted: "已提交",
  under_review: "审核中",
  approved: "已批准",
  changes_requested: "要求修改",
  rejected: "已拒绝",
  materialized: "已发布",
  internal: "内部",
  private: "私密",
  public: "公开",
  normal: "普通风险",
  elevated: "较高风险",
  high: "高风险",
  private_memory: "私有记忆",
  file_snapshot: "文件快照",
  git_object: "Git 对象",
  test_receipt: "测试收据",
  external_document: "外部文档",
};

function announce(message) {
  const node = $("#status");
  node.textContent = message;
  node.classList.add("visible");
  window.setTimeout(() => node.classList.remove("visible"), 3200);
}

async function request(path, options = {}) {
  let response;
  try {
    response = await fetch(path, options);
  } catch (error) {
    enterExpired("ART 本地审核服务已断开。请让 Agent 重新打开 ART 治理页面。");
    throw error;
  }
  const payload = await response.json().catch(() => ({}));
  if (!response.ok) {
    if (response.status === 401 || response.status === 403) {
      enterExpired("此审核会话已过期或不再获得授权。你仍可复制已输入的审核理由，然后让 Agent 重新打开 ART 治理页面。");
    } else if (response.status === 409) {
      enterConflict("提案版本、状态或哈希已变化。当前决定没有提交，请重新打开最新版本。");
    }
    const error = new Error(payload.error || `请求失败 (${response.status})`);
    error.status = response.status;
    throw error;
  }
  return payload;
}

function setView(view) {
  document.querySelectorAll(".view").forEach((node) => {
    node.hidden = node.id !== `${view}-view`;
  });
  document.querySelectorAll("[data-view]").forEach((node) => {
    if (node.dataset.view === view) node.setAttribute("aria-current", "page");
    else node.removeAttribute("aria-current");
  });
}

function badge(text, kind) {
  const node = document.createElement("span");
  node.className = `badge ${kind || ""}`;
  node.textContent = labels[text] || text;
  return node;
}

function renderProposals() {
  const list = $("#proposal-list");
  const proposals = ui.bootstrap.proposals;
  $("#queue-count").textContent = String(ui.bootstrap.actionable_count);
  list.setAttribute("aria-busy", "false");
  if (!proposals.length) {
    const empty = document.createElement("p");
    empty.className = "empty";
    empty.textContent = "目前没有等待处理的知识提案。";
    list.replaceChildren(empty);
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
    const tags = document.createElement("div");
    tags.className = "label-row";
    tags.append(badge(proposal.status, "status-badge"), badge(proposal.sensitivity), badge(proposal.risk, `risk-${proposal.risk}`));
    const meta = document.createElement("p");
    meta.className = "proposal-meta";
    meta.textContent = `${proposal.proposal_id}@${proposal.revision} · 更新于 ${new Date(proposal.updated_at).toLocaleString()}`;
    copy.append(title, applicability, tags, meta);
    const review = document.createElement("button");
    review.type = "button";
    review.className = "action primary review-details";
    review.textContent = "审核详情";
    review.addEventListener("click", () => openProposal(proposal, review));
    card.append(copy, review);
    return card;
  }));
}

function renderAudit() {
  const list = $("#audit-list");
  if (!ui.bootstrap.audit.length) {
    const item = document.createElement("li");
    item.className = "empty";
    item.textContent = "尚无 Agent 委托治理记录。";
    list.replaceChildren(item);
    return;
  }
  list.replaceChildren(...ui.bootstrap.audit.map((entry) => {
    const item = document.createElement("li");
    const time = document.createElement("time");
    time.dateTime = entry.created_at;
    time.textContent = new Date(entry.created_at).toLocaleString();
    const line = document.createElement("p");
    line.textContent = `${entry.operation === "approve" ? "批准" : "发布"} · ${entry.actor_type}`;
    const detail = document.createElement("small");
    detail.textContent = `${entry.proposal_id}@${entry.revision} · ${entry.agent_id} · host ${entry.host_binding}`;
    item.append(time, line, detail);
    return item;
  }));
}

async function refresh() {
  ui.bootstrap = await request(`/api/bootstrap?session=${encodeURIComponent(session || "")}`);
  $("#connection-dot").classList.add("ready");
  $("#agent-label").textContent = `${ui.bootstrap.bound_agent_id} · host ${ui.bootstrap.host_binding}`;
  const toggle = $("#delegation-toggle");
  toggle.checked = ui.bootstrap.governance_mode === "delegated_local";
  $("#policy-explanation").textContent = toggle.checked
    ? "已开启。明确的当前用户指令可由此 Agent 一次完成批准与发布。"
    : "已关闭。提案必须在此页面由人类审核和发布。";
  renderProposals();
  renderAudit();
  setView(ui.bootstrap.view || "pending");
  updateSessionClock();
  if (!ui.autoOpened && ui.bootstrap.proposal_id && ui.bootstrap.revision) {
    const proposal = ui.bootstrap.proposals.find((item) => item.proposal_id === ui.bootstrap.proposal_id && item.revision === ui.bootstrap.revision);
    if (proposal) {
      ui.autoOpened = true;
      const trigger = document.querySelector(".review-details");
      await openProposal(proposal, trigger);
    }
  }
}

async function mutate(path, body) {
  return request(path, {
    method: "POST",
    headers: { "content-type": "application/json", "x-art-csrf": ui.bootstrap.csrf_token },
    body: JSON.stringify({ session, ...body }),
  });
}

async function openProposal(proposal, trigger) {
  ui.trigger = trigger || document.activeElement;
  ui.detail = null;
  ui.decision = null;
  ui.conflict = false;
  $("#detail-state").hidden = false;
  $("#detail-state").className = "notice";
  $("#detail-state").textContent = "正在读取完整提案、锁定来源与当前 Edition…";
  $("#detail-content").hidden = true;
  $("#drawer-footer").hidden = true;
  $("#session-warning").hidden = true;
  if (!dialog.open) dialog.showModal();
  $("#close-detail").focus();
  announce("正在读取提案详情");
  try {
    await loadProposalDetail(proposal.proposal_id, proposal.revision);
  } catch (error) {
    if (!ui.expired && !ui.conflict) {
      $("#detail-state").textContent = error.message;
      $("#detail-state").className = "notice error";
    }
  }
}

async function loadProposalDetail(id, revision) {
  const query = new URLSearchParams({ session: session || "", proposal_id: id, revision: String(revision) });
  const detail = await request(`/api/proposal-detail?${query.toString()}`);
  ui.detail = detail;
  ui.expired = false;
  ui.conflict = false;
  renderDetail(detail);
  $("#detail-state").hidden = true;
  $("#detail-content").hidden = false;
  $("#drawer-footer").hidden = false;
  announce("提案详情已载入");
}

function addDefinition(list, term, value) {
  const dt = document.createElement("dt");
  dt.textContent = term;
  const dd = document.createElement("dd");
  dd.textContent = value == null || value === "" ? "—" : String(value);
  list.append(dt, dd);
}

function renderDetail(detail) {
  const proposal = detail.proposal;
  $("#proposal-title").textContent = proposal.title;
  $("#proposal-description").textContent = `${proposal.proposal_id}@${proposal.revision} · 精确审核快照`;
  $("#proposal-labels").replaceChildren(
    badge(proposal.status, "status-badge"),
    badge(proposal.sensitivity),
    badge(proposal.risk, `risk-${proposal.risk}`),
  );
  const metadata = $("#proposal-metadata");
  metadata.replaceChildren();
  [
    ["提案 ID", proposal.proposal_id],
    ["精确版本", proposal.revision],
    ["作者", proposal.author_agent_id],
    ["创建时间", new Date(proposal.created_at).toLocaleString()],
    ["更新时间", new Date(proposal.updated_at).toLocaleString()],
    ["草稿 SHA-256", proposal.draft_hash],
    ["来源集 SHA-256", proposal.source_set_hash],
  ].forEach(([term, value]) => addDefinition(metadata, term, value));

  $("#knowledge-panel").innerHTML = detail.content.rendered_html;
  $("#raw-markdown").textContent = detail.content.canonical_markdown;
  renderComparison(detail.comparison);
  renderSources(detail.sources);
  renderCurrentEdition(detail.current_edition);
  renderReviews(detail.reviews);
  renderRequirements(detail.requirements);
  renderActions(detail.allowed_actions);
  updateProvenance(proposal.status);
  resetInlinePanels();
  selectContentTab($("#knowledge-tab"));
  $("#detail-content").scrollTop = 0;
}

function renderComparison(comparison) {
  const node = $("#comparison");
  if (comparison.kind === "first_edition") {
    const empty = document.createElement("p");
    empty.className = "empty";
    empty.textContent = "这是该知识键的首个 Edition；全部内容均为新增，没有可比较的早期版本。";
    node.replaceChildren(empty);
    return;
  }
  const fragment = document.createDocumentFragment();
  comparison.groups.forEach((group, index) => {
    const section = document.createElement("section");
    section.className = "diff-group";
    section.setAttribute("aria-label", `差异区段 ${index + 1}`);
    group.forEach((line) => {
      const row = document.createElement("div");
      row.className = `diff-line ${line.kind}`;
      const label = document.createElement("span");
      label.className = "diff-label";
      label.textContent = { added: "新增", removed: "删除", unchanged: "未变" }[line.kind];
      const number = document.createElement("span");
      number.className = "diff-number";
      number.textContent = `${line.old_line || "–"} / ${line.new_line || "–"}`;
      const content = line.kind === "added" ? document.createElement("ins") : line.kind === "removed" ? document.createElement("del") : document.createElement("span");
      content.textContent = line.text || " ";
      row.append(label, number, content);
      section.append(row);
    });
    fragment.append(section);
  });
  node.replaceChildren(fragment);
}

function renderSources(sources) {
  $("#source-records").replaceChildren(...sources.map((source) => {
    const card = document.createElement("article");
    card.className = "evidence-card source-card";
    const title = document.createElement("h4");
    title.textContent = `${labels[source.source_type] || source.source_type} · ${source.source_id}`;
    const data = document.createElement("dl");
    data.className = "metadata-grid compact";
    [
      ["来源版本", source.source_revision],
      ["内容 SHA-256", source.source_content_hash],
      ["锚点集 SHA-256", source.anchor_set_hash],
      ["批准摘录 SHA-256", source.approved_excerpt_hash],
      ["适用授权", source.use_grant_id],
    ].forEach(([term, value]) => addDefinition(data, term, value));
    card.append(title, data);
    return card;
  }));
}

function renderCurrentEdition(edition) {
  const node = $("#current-edition");
  if (!edition) {
    node.textContent = "尚无当前 Edition。本提案发布后将成为 Edition 1。";
    return;
  }
  const data = document.createElement("dl");
  data.className = "metadata-grid compact";
  [
    ["Edition", `${edition.edition_id} · #${edition.edition_number}`],
    ["发布时间", new Date(edition.published_at).toLocaleString()],
    ["Markdown SHA-256", edition.markdown_sha256],
    ["Manifest SHA-256", edition.manifest_sha256],
  ].forEach(([term, value]) => addDefinition(data, term, value));
  node.replaceChildren(data);
}

function renderReviews(reviews) {
  const node = $("#review-history");
  if (!reviews.length) {
    const item = document.createElement("li");
    item.className = "empty";
    item.textContent = "此版本尚无审核决定。";
    node.replaceChildren(item);
    return;
  }
  node.replaceChildren(...reviews.map((review) => {
    const item = document.createElement("li");
    const heading = document.createElement("strong");
    heading.textContent = `${labels[review.decision] || review.decision} · ${review.actor}`;
    const time = document.createElement("time");
    time.dateTime = review.decided_at;
    time.textContent = new Date(review.decided_at).toLocaleString();
    const reason = document.createElement("p");
    reason.textContent = review.reason;
    item.append(heading, time, reason);
    return item;
  }));
}

function renderRequirements(requirements) {
  const node = $("#requirements");
  node.classList.toggle("warning-card", requirements.independent_review_required);
  node.textContent = requirements.independent_review_required
    ? "需要一名不同审核者完成独立审核。当前本地页面没有可证明的第二个人类身份，因此不会降低该规则。"
    : "当前提案已满足进入下一项可用决定的治理要求。";
}

function renderActions(actions) {
  const node = $("#detail-actions");
  const names = { approved: "批准", changes_requested: "要求修改", rejected: "拒绝", publish: "准备发布" };
  node.replaceChildren(...actions.map((action) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = `action ${action === "approved" || action === "publish" ? "primary" : ""} ${action === "rejected" ? "danger" : ""}`;
    button.textContent = names[action];
    button.addEventListener("click", () => action === "publish" ? openPublishConfirmation() : openDecisionForm(action));
    return button;
  }));
  $("#action-context").textContent = actions.length
    ? `只会提交 ${ui.detail.proposal.proposal_id}@${ui.detail.proposal.revision}。`
    : "此版本没有可执行的治理操作。";
}

function updateProvenance(status) {
  const reached = status === "approved" ? 2 : status === "materialized" ? 3 : 1;
  document.querySelectorAll("#provenance-spine li").forEach((node, index) => {
    node.classList.toggle("complete", index <= reached);
    node.setAttribute("aria-current", index === reached ? "step" : "false");
  });
}

function resetInlinePanels() {
  $("#decision-form").hidden = true;
  $("#publish-confirmation").hidden = true;
  $("#publication-receipt").hidden = true;
  $("#review-result").hidden = true;
  $("#review-error").hidden = true;
  $("#review-reason").removeAttribute("aria-invalid");
  $("#review-reason").readOnly = false;
  ui.decision = null;
}

function openDecisionForm(decision) {
  if (!canMutate()) return;
  ui.decision = decision;
  const names = { approved: "批准", changes_requested: "要求修改", rejected: "拒绝" };
  $("#decision-legend").textContent = `${names[decision]}此提案版本`;
  $("#decision-target").textContent = `${ui.detail.proposal.proposal_id}@${ui.detail.proposal.revision}`;
  $("#review-reason").value = "";
  $("#review-error").hidden = true;
  $("#decision-form").hidden = false;
  $("#publish-confirmation").hidden = true;
  $("#decision-form").scrollIntoView({ block: "nearest" });
  $("#review-reason").focus();
}

async function submitReview() {
  if (!canMutate()) return;
  const field = $("#review-reason");
  const reason = field.value.trim();
  if (reason.length < 1 || [...reason].length > 1000) {
    field.setAttribute("aria-invalid", "true");
    $("#review-error").textContent = "请输入 1–1,000 个字符的审核理由。";
    $("#review-error").hidden = false;
    field.focus();
    return;
  }
  setBusy(true);
  const proposal = ui.detail.proposal;
  try {
    const receipt = await mutate("/api/review", {
      proposal_id: proposal.proposal_id,
      revision: proposal.revision,
      status: proposal.status,
      draft_hash: proposal.draft_hash,
      source_set_hash: proposal.source_set_hash,
      decision: ui.decision,
      reason,
    });
    await refresh();
    await loadProposalDetail(proposal.proposal_id, proposal.revision);
    const result = $("#review-result");
    result.textContent = `决定已记录：${labels[receipt.decision] || receipt.decision}，当前状态为 ${labels[receipt.status] || receipt.status}。`;
    result.hidden = false;
    result.scrollIntoView({ block: "nearest" });
    result.focus({ preventScroll: true });
    announce("审核决定已提交");
  } catch (error) {
    if (!ui.expired && !ui.conflict) {
      $("#review-error").textContent = error.message;
      $("#review-error").hidden = false;
    }
  } finally {
    setBusy(false);
  }
}

function openPublishConfirmation() {
  if (!canMutate()) return;
  $("#decision-form").hidden = true;
  const summary = $("#publish-summary");
  summary.replaceChildren();
  const proposal = ui.detail.proposal;
  [
    ["精确提案", `${proposal.proposal_id}@${proposal.revision}`],
    ["草稿 SHA-256", proposal.draft_hash],
    ["来源集 SHA-256", proposal.source_set_hash],
    ["下一 Edition", ui.detail.predicted_edition_number],
  ].forEach(([term, value]) => addDefinition(summary, term, value));
  $("#publish-confirmation").hidden = false;
  $("#publish-confirmation").scrollIntoView({ block: "nearest" });
  $("#confirm-publish").focus();
}

async function submitPublish() {
  if (!canMutate()) return;
  setBusy(true);
  const proposal = ui.detail.proposal;
  try {
    const receipt = await mutate("/api/publish", {
      proposal_id: proposal.proposal_id,
      revision: proposal.revision,
      status: proposal.status,
      draft_hash: proposal.draft_hash,
      source_set_hash: proposal.source_set_hash,
      predicted_edition_number: ui.detail.predicted_edition_number,
      confirm: true,
    });
    const fields = $("#receipt-fields");
    fields.replaceChildren();
    [
      ["Edition ID", receipt.edition_id],
      ["Edition number", receipt.edition_number],
      ["发布时间", new Date(receipt.published_at).toLocaleString()],
      ["Markdown SHA-256", receipt.markdown_sha256],
      ["Manifest SHA-256", receipt.manifest_sha256],
    ].forEach(([term, value]) => addDefinition(fields, term, value));
    $("#publish-confirmation").hidden = true;
    $("#publication-receipt").hidden = false;
    $("#drawer-footer").hidden = true;
    updateProvenance("materialized");
    await refresh();
    $("#publication-receipt").scrollIntoView({ block: "nearest" });
    setBusy(false);
    $("#close-receipt").focus();
    announce("Edition 已发布，收据已生成");
  } catch (error) {
    if (!ui.expired && !ui.conflict) announce(error.message);
  } finally {
    if (ui.busy) setBusy(false);
  }
}

function setBusy(busy) {
  ui.busy = busy;
  dialog.setAttribute("aria-busy", String(busy));
  dialog.querySelectorAll("button").forEach((control) => {
    if (control.id !== "close-detail") control.disabled = busy || ui.expired || ui.conflict;
  });
  $("#review-reason").readOnly = busy || ui.expired || ui.conflict;
  $("#close-detail").disabled = busy;
}

function canMutate() {
  return ui.detail && !ui.busy && !ui.expired && !ui.conflict;
}

function enterConflict(message) {
  ui.conflict = true;
  if (!dialog.open) return;
  $("#detail-state").hidden = false;
  $("#detail-state").className = "notice error";
  $("#detail-state").textContent = message;
  $("#drawer-footer").hidden = true;
  dialog.querySelectorAll("button").forEach((control) => {
    if (control.id !== "close-detail") control.disabled = true;
  });
  $("#review-reason").readOnly = true;
  announce("提案发生冲突，当前决定未提交");
}

function enterExpired(message) {
  ui.expired = true;
  $("#connection-dot").classList.remove("ready");
  if (dialog.open) {
    $("#detail-state").hidden = false;
    $("#detail-state").className = "notice error persistent";
    $("#detail-state").textContent = message;
    $("#drawer-footer").hidden = true;
    dialog.querySelectorAll("button").forEach((control) => {
      if (control.id !== "close-detail") control.disabled = true;
    });
    $("#review-reason").readOnly = true;
  }
  announce("审核会话已结束");
}

function canCloseDetail() {
  if (ui.busy) return false;
  const hasDraftReason = !$("#decision-form").hidden && $("#review-reason").value.trim().length > 0;
  const hasPublishConfirmation = !$("#publish-confirmation").hidden;
  if (hasDraftReason || hasPublishConfirmation) {
    announce("先取消当前决定或发布确认，再关闭详情。已输入的理由仍保留在页面中。");
    return false;
  }
  return true;
}

function closeDetail(force = false) {
  if (!force && !canCloseDetail()) return;
  dialog.close();
  const trigger = ui.trigger;
  ui.detail = null;
  ui.decision = null;
  ui.trigger = null;
  if (trigger && trigger.isConnected) trigger.focus();
  else $("#workspace").focus({ preventScroll: true });
}

function selectContentTab(tab) {
  const tabs = [...document.querySelectorAll('[role="tab"]')];
  tabs.forEach((item) => {
    const selected = item === tab;
    item.setAttribute("aria-selected", String(selected));
    item.tabIndex = selected ? 0 : -1;
    $(`#${item.getAttribute("aria-controls")}`).hidden = !selected;
  });
}

function updateSessionClock() {
  if (!ui.bootstrap) return;
  const remaining = new Date(ui.bootstrap.expires_at).getTime() - Date.now();
  const seconds = Math.max(0, Math.ceil(remaining / 1000));
  const minutes = Math.floor(seconds / 60);
  const label = `${minutes}:${String(seconds % 60).padStart(2, "0")} 后过期`;
  $("#session-expiry").textContent = `本地会话 · ${label}`;
  $("#drawer-time").textContent = label;
  if (remaining <= 5 * 60 * 1000 && remaining > 0) {
    $("#session-warning").hidden = false;
    if (!ui.warned) {
      ui.warned = true;
      announce("审核会话还剩五分钟");
    }
  }
  if (remaining <= 0 && !ui.expired) {
    enterExpired("此审核会话已过期。你仍可复制已输入的审核理由，然后让 Agent 重新打开 ART 治理页面。");
  }
}

document.querySelectorAll("[data-view]").forEach((button) => {
  button.addEventListener("click", () => setView(button.dataset.view));
});
document.querySelectorAll('[role="tab"]').forEach((tab) => {
  tab.addEventListener("click", () => selectContentTab(tab));
  tab.addEventListener("keydown", (event) => {
    const tabs = [...document.querySelectorAll('[role="tab"]')];
    const index = tabs.indexOf(tab);
    let next = null;
    if (event.key === "ArrowRight") next = tabs[(index + 1) % tabs.length];
    if (event.key === "ArrowLeft") next = tabs[(index - 1 + tabs.length) % tabs.length];
    if (event.key === "Home") next = tabs[0];
    if (event.key === "End") next = tabs[tabs.length - 1];
    if (next) {
      event.preventDefault();
      selectContentTab(next);
      next.focus();
    }
  });
});

$("#delegation-toggle").addEventListener("change", async (event) => {
  const mode = event.target.checked ? "delegated_local" : "human_review";
  event.target.disabled = true;
  try {
    await mutate("/api/delegation", { mode });
    announce(mode === "delegated_local" ? "Agent 委托治理已开启" : "Agent 委托治理已关闭");
    await refresh();
  } catch (error) {
    event.target.checked = !event.target.checked;
    announce(error.message);
  } finally {
    event.target.disabled = false;
  }
});

$("#decision-form").addEventListener("submit", (event) => {
  event.preventDefault();
  submitReview();
});
$("#cancel-decision").addEventListener("click", () => {
  $("#review-reason").value = "";
  $("#decision-form").hidden = true;
  ui.decision = null;
  $("#detail-actions button").focus();
});
$("#confirm-publish").addEventListener("click", submitPublish);
$("#cancel-publish").addEventListener("click", () => {
  $("#publish-confirmation").hidden = true;
  $("#detail-actions button").focus();
});
$("#close-detail").addEventListener("click", () => closeDetail());
$("#close-receipt").addEventListener("click", () => closeDetail(true));
dialog.addEventListener("cancel", (event) => {
  event.preventDefault();
  closeDetail();
});
dialog.addEventListener("keydown", (event) => {
  if (event.key !== "Tab") return;
  const focusable = [...dialog.querySelectorAll('button:not([disabled]), textarea:not([disabled]), [tabindex="0"]')]
    .filter((node) => !node.hidden && node.offsetParent !== null);
  if (!focusable.length) return;
  const first = focusable[0];
  const last = focusable[focusable.length - 1];
  if (event.shiftKey && document.activeElement === first) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && document.activeElement === last) {
    event.preventDefault();
    first.focus();
  }
});

window.setInterval(updateSessionClock, 1000);
refresh().catch((error) => {
  $("#agent-label").textContent = "本地会话不可用";
  announce(error.message);
});
