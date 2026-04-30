import {
  LANGUAGES,
  absenceKindLabel,
  getLanguage,
  getLocale,
  roleLabel,
  setLanguage,
  statusLabel,
  t,
} from "./i18n.js";

// KitaZeit SPA — vanilla JS

const API = "/api/v1";
let CURRENT_USER = null;
let CATEGORIES = [];
let CSRF_TOKEN = null;
let APP_SETTINGS = { ui_language: getLanguage() };
let SETTINGS_PROMISE = null;

const initialApp = document.getElementById("app");
if (initialApp) initialApp.textContent = t("Loading...");

function h(tag, attrs={}, ...children){
  const el = document.createElement(tag);
  for (const [k,v] of Object.entries(attrs||{})){
    if (v == null || v === false) continue;
    if (k === "class") el.className = v;
    else if (k === "html") el.innerHTML = v;
    else if (k.startsWith("on") && typeof v === "function") el.addEventListener(k.slice(2), v);
    else if (k === "style" && typeof v === "object") Object.assign(el.style, v);
    else el.setAttribute(k, v);
  }
  for (const c of children.flat()){
    if (c == null || c === false) continue;
    el.appendChild(c.nodeType ? c : document.createTextNode(String(c)));
  }
  return el;
}

async function api(path, opts={}){
  const headers = opts.body ? { "Content-Type": "application/json" } : {};
  const method = (opts.method || "GET").toUpperCase();
  if (CSRF_TOKEN && method !== "GET" && method !== "HEAD" && method !== "OPTIONS"){
    headers["X-CSRF-Token"] = CSRF_TOKEN;
  }
  const r = await fetch(API + path, {
    headers,
    credentials: "same-origin",
    ...opts,
    body: opts.body ? JSON.stringify(opts.body) : undefined
  });
  if (r.status === 204) return null;
  const ct = r.headers.get("content-type") || "";
  if (!ct.includes("json")){
    if (!r.ok) throw new Error(await r.text() || "Error");
    return r;
  }
  const d = await r.json();
  if (!r.ok) throw new Error(d.error || "Error");
  return d;
}

function toast(msg, type="info"){
  const el = h("div", {class:"toast " + type}, msg);
  document.body.appendChild(el);
  setTimeout(() => el.remove(), 3500);
}

function fmtDate(d){ const x = new Date(d); return x.toLocaleDateString(getLocale(),{weekday:"short",day:"2-digit",month:"2-digit",year:"numeric"}); }
function fmtDateShort(d){ const x = new Date(d); return x.toLocaleDateString(getLocale(),{day:"2-digit",month:"2-digit"}); }
function fmtMonthYear(d){ const x = new Date(d); return x.toLocaleDateString(getLocale(), {month:"long", year:"numeric"}); }
function fmtDateTime(d){ const x = new Date(d); return x.toLocaleString(getLocale()); }
function fmtWeekdayLong(d){ const x = new Date(d); return x.toLocaleDateString(getLocale(), {weekday:"long"}); }
function weekdayLabels(){
  const base = new Date(Date.UTC(2024, 0, 1));
  return Array.from({length: 7}, (_, index) =>
    new Date(base.getTime() + index * 86400000).toLocaleDateString(getLocale(), {
      weekday:"short",
      timeZone:"UTC"
    })
  );
}
function isoDate(d){ const x = new Date(d); return x.getFullYear() + "-" + String(x.getMonth()+1).padStart(2,"0") + "-" + String(x.getDate()).padStart(2,"0"); }
function monday(d){ const x = new Date(d); const wd = (x.getDay()+6)%7; x.setDate(x.getDate()-wd); x.setHours(0,0,0,0); return x; }
function addDays(d, n){ const x = new Date(d); x.setDate(x.getDate()+n); return x; }
function minToHM(min){ const sign = min<0?"-":""; const a = Math.abs(min); const h = Math.floor(a/60); const m = a%60; return sign + h + ":" + String(m).padStart(2,"0"); }
function durMin(start, end){
  const [bh,bm] = start.split(":").map(Number);
  const [eh,em] = end.split(":").map(Number);
  return (eh*60+em) - (bh*60+bm);
}

const routes = [];
function route(pattern, handler){ routes.push({pattern, handler}); }

function go(path, push=true){
  if (push) history.pushState({}, "", path);
  else history.replaceState({}, "", path);
  render();
}

function errorMessage(error){
  return t(error?.message || "Error");
}

function applySettings(settings = {}){
  APP_SETTINGS = { ...APP_SETTINGS, ...settings };
  if (APP_SETTINGS.ui_language) setLanguage(APP_SETTINGS.ui_language);
  const app = document.getElementById("app");
  if (app && !app.children.length) app.textContent = t("Loading...");
  return APP_SETTINGS;
}

async function ensurePublicSettings(force=false){
  if (!SETTINGS_PROMISE || force){
    SETTINGS_PROMISE = api("/settings/public")
      .then(applySettings)
      .catch(() => APP_SETTINGS);
  }
  return await SETTINGS_PROMISE;
}

window.addEventListener("popstate", () => render());

document.addEventListener("click", e => {
  const a = e.target.closest("a[data-link]");
  if (a){ e.preventDefault(); go(a.getAttribute("href")); }
});

function matchRoute(pattern, path){
  const a = pattern.split("/").filter(Boolean);
  const b = path.split("/").filter(Boolean);
  if (a.length !== b.length) return null;
  const params = {};
  for (let i=0; i<a.length; i++){
    if (a[i].startsWith(":")) params[a[i].slice(1)] = b[i];
    else if (a[i] !== b[i]) return null;
  }
  return params;
}

async function render(){
  await ensurePublicSettings();
  const path = location.pathname;
  if (CURRENT_USER === null){
    try {
      CURRENT_USER = await api("/auth/me");
      if (CURRENT_USER && CURRENT_USER.csrf_token) CSRF_TOKEN = CURRENT_USER.csrf_token;
      if (CURRENT_USER && CURRENT_USER.csrf_token) CSRF_TOKEN = CURRENT_USER.csrf_token;
    } catch { CURRENT_USER = false; }
  }
  if (!CURRENT_USER && path !== "/login"){ go("/login", false); return; }
  if (CURRENT_USER && path === "/login"){ go("/", false); return; }
  if (CURRENT_USER && path === "/"){
    go(CURRENT_USER.role === "employee" ? "/time" : "/dashboard", false); return;
  }
  if (CATEGORIES.length === 0 && CURRENT_USER){
    try { CATEGORIES = await api("/categories"); } catch {}
  }
  const app = document.getElementById("app");
  if (path === "/login"){ app.innerHTML = ""; app.appendChild(loginView()); return; }
  if (CURRENT_USER && CURRENT_USER.must_change_password && path !== "/account"){
    go("/account", false);
    toast(t("Please change your temporary password."), "error");
    return;
  }
  const hit = routes.find(r => matchRoute(r.pattern, path));
  app.innerHTML = "";
  app.appendChild(layout(hit ? await hit.handler(matchRoute(hit.pattern, path)) : notFound()));
}

function layout(content){
  const u = CURRENT_USER;
  const lead = u.role === "team_lead" || u.role === "admin";
  const admin = u.role === "admin";
  const links = [
    {href:"/time", icon:"⏱", text:"Time"},
    {href:"/absences", icon:"📅", text:"Absences"},
    {href:"/calendar", icon:"🗓", text:"Calendar"},
    {href:"/account", icon:"👤", text:"Account"},
  ];
  if (lead) links.push({href:"/dashboard", icon:"🔔", text:"Dashboard"});
  if (lead) links.push({href:"/reports", icon:"📊", text:"Reports"});
  if (admin) links.push({href:"/admin/users", icon:"⚙", text:"Admin"});

  const nav = h("nav", {class:"nav"},
    h("div", {class:"brand"},
      h("span", {class:"logo"}, "KZ"),
      h("span", {}, "KitaZeit")
    ),
    ...links.map(l => h("a", {href: l.href, "data-link": "1", class: location.pathname === l.href || location.pathname.startsWith(l.href + "/") ? "active" : ""},
      h("span", {class:"icon"}, l.icon), h("span", {class:"label"}, t(l.text))
    )),
    h("div", {class:"footer"},
      h("div", {class:"who"}, u.first_name + " " + u.last_name),
      h("div", {}, h("span", {class:"role"}, roleLabel(u.role))),
      h("a", {href:"#", onclick: async (e) => { e.preventDefault(); await api("/auth/logout",{method:"POST"}); CURRENT_USER=null; go("/login"); }}, t("Sign out"))
    )
  );
  return h("div", {class:"layout"}, nav, h("main", {}, content));
}

function notFound(){ return h("div", {class:"card"}, h("h1", {}, t("Page not found"))); }
function notAllowed(){ return h("div", {class:"card"}, h("h1", {}, t("Forbidden"))); }

function field(label, input){
  if (input.name && !input.id) input.id = "f-" + input.name;
  const lbl = h("label", input.id ? {for:input.id} : {}, label);
  return h("div", {class:"field"}, lbl, input);
}

function loginView(){
  const wrap = h("div", {class:"login-wrap"});
  const card = h("div", {class:"card"});
  card.appendChild(h("div", {class:"brand-row"},
    h("span", {class:"logo"}, "KZ"),
    h("h1", {}, "KitaZeit")
  ));
  card.appendChild(h("p", {class:"muted"}, t("Sign in to your time-tracking workspace.")));
  const form = h("form", {});
  const errEl = h("div", {class:"error"});
  form.appendChild(field(t("Email"), h("input", {type:"email", name:"email", required:"1", autocomplete:"email"})));
  form.appendChild(field(t("Password"), h("input", {type:"password", name:"password", required:"1", autocomplete:"current-password"})));
  form.appendChild(errEl);
  form.appendChild(h("button", {type:"submit"}, t("Sign in")));
  form.addEventListener("submit", async e => {
    e.preventDefault();
    errEl.textContent = "";
    const fd = new FormData(form);
    try {
      const r = await api("/auth/login", { method:"POST", body:{ email: fd.get("email"), password: fd.get("password") }});
      CURRENT_USER = r.user; CURRENT_USER.must_change_password = r.must_change_password;
      CSRF_TOKEN = r.csrf_token || null;
      CATEGORIES = [];
      go("/");
    } catch (err){ errEl.textContent = errorMessage(err); }
  });
  card.appendChild(form);
  wrap.appendChild(card);
  return wrap;
}

function confirmDialog(title, text, opts={}){
  return new Promise(resolve => {
    const dlg = h("dialog", {});
    dlg.appendChild(h("header", {}, t(title)));
    const body = h("div", {class:"inner"}, text ? h("p", {}, t(text)) : null);
    let ta;
    if (opts.reason){
      ta = h("textarea", {rows:"3", placeholder:t("Reason"), required:"1"});
      body.appendChild(field(t("Reason"), ta));
    }
    dlg.appendChild(body);
    dlg.appendChild(h("footer", {},
      h("button", {class:"sec", onclick:()=>{dlg.close(); resolve(null); dlg.remove();}}, t("Cancel")),
      h("button", {class: opts.danger?"danger":"", onclick:()=>{
        if (opts.reason && !ta.value.trim()){ toast(t("Reason required"),"error"); return; }
        dlg.close(); resolve(opts.reason ? ta.value : true); dlg.remove();
      }}, t(opts.confirm || "OK"))
    ));
    document.body.appendChild(dlg); dlg.showModal();
  });
}

function isoWeek(d){
  const t = new Date(Date.UTC(d.getFullYear(), d.getMonth(), d.getDate()));
  const dn = (t.getUTCDay()+6)%7;
  t.setUTCDate(t.getUTCDate() - dn + 3);
  const j1 = new Date(Date.UTC(t.getUTCFullYear(),0,4));
  return 1 + Math.round(((t-j1)/86400000 - 3 + ((j1.getUTCDay()+6)%7))/7);
}

// --- Time view ---
route("/time", async () => {
  const params = new URLSearchParams(location.search);
  const date = params.get("week") ? new Date(params.get("week")) : new Date();
  const mo = monday(date);
  const su = addDays(mo, 6);
  const entries = await api(`/time-entries?from=${isoDate(mo)}&to=${isoDate(su)}`);
  const wrap = h("div");
  wrap.appendChild(h("h1", {}, t("Time tracking")));
  const nav = h("div", {class:"row", style:{marginBottom:"1em"}},
    h("button", {class:"sec", onclick:()=>go("/time?week="+isoDate(addDays(mo,-7)))}, `← ${t("Previous week")}`),
    h("strong", {}, t("Week {week}: {from} - {to}", {week: isoWeek(mo), from: fmtDateShort(mo), to: fmtDateShort(su)})),
    h("button", {class:"sec", onclick:()=>go("/time?week="+isoDate(addDays(mo,7)))}, `${t("Next week")} →`),
    h("button", {class:"sec", onclick:async ()=>{
      const v = await api(`/time-entries?from=${isoDate(addDays(mo,-7))}&to=${isoDate(addDays(mo,-1))}`);
      let n = 0;
      for (const e of v){
        const newDate = isoDate(addDays(new Date(e.entry_date), 7));
        if (new Date(newDate) > new Date()) continue;
        try {
          await api("/time-entries", { method:"POST", body:{ entry_date: newDate, start_time: e.start_time.slice(0,5), end_time: e.end_time.slice(0,5), category_id: e.category_id, comment: e.comment }});
          n++;
        } catch {}
      }
      toast(t("Copied {count} entries.", {count: n}), "ok"); render();
    }}, t("Copy last week"))
  );
  wrap.appendChild(nav);

  const weekActual = entries.reduce((s,e)=> s + durMin(e.start_time.slice(0,5), e.end_time.slice(0,5)), 0);
  const weekTarget = Math.round(CURRENT_USER.weekly_hours * 60);
  wrap.appendChild(h("div", {class:"kpi"},
    h("div", {class:"box"}, h("div",{class:"label"},t("Target")), h("div",{class:"val"},minToHM(weekTarget))),
    h("div", {class:"box"}, h("div",{class:"label"},t("Actual")), h("div",{class:"val"},minToHM(weekActual))),
    h("div", {class:"box"}, h("div",{class:"label"},t("Difference")), h("div",{class:"val", style:{color: weekActual-weekTarget<0?"var(--danger)":"var(--success)"}}, minToHM(weekActual-weekTarget)))
  ));

  for (let i=0; i<7; i++){
    const d = addDays(mo, i);
    const ds = isoDate(d);
    const dayEntries = entries.filter(e => e.entry_date === ds).sort((a,b)=> a.start_time.localeCompare(b.start_time));
    if (i >= 5 && dayEntries.length === 0) continue;
    const block = h("div", {class:"dayblock"});
    const total = dayEntries.reduce((s,e)=> s + durMin(e.start_time.slice(0,5), e.end_time.slice(0,5)), 0);
    block.appendChild(h("div", {class:"row"}, h("h3", {style:{flex:"1"}}, fmtDate(d)), h("span", {class:"total"}, "Σ " + minToHM(total))));
    for (const e of dayEntries){
      const c = CATEGORIES.find(x => x.id === e.category_id) || {name:"?", color:"#999"};
      const m = durMin(e.start_time.slice(0,5), e.end_time.slice(0,5));
      const row = h("div", {class:"entry"},
        h("span", {class:"cat-bar", style:{background:c.color}}),
        h("span", {class:"time"}, e.start_time.slice(0,5) + " – " + e.end_time.slice(0,5)),
        h("span", {class:"cat"}, c.name + (e.comment ? " · " + e.comment : "")),
        h("span", {class:"chip "+e.status}, statusLabel(e.status)),
        h("span", {style:{minWidth:"60px",textAlign:"right"}}, minToHM(m)),
        e.status === "draft" ? h("button", {class:"sec", onclick:()=>entryDialog(e)}, t("Edit")) : null,
        e.status === "draft" ? h("button", {class:"danger", onclick:async ()=>{
          if (!await confirmDialog("Delete?", "Delete this entry?", {danger:true, confirm:"Delete"})) return;
          await api("/time-entries/"+e.id, {method:"DELETE"}); render();
        }}, t("Delete")) : null,
        (e.status === "submitted" || e.status === "approved") ? h("button", {class:"sec", onclick:()=>changeRequestDialog(e)}, t("Request change")) : null,
        e.status === "rejected" && e.rejection_reason ? h("span", {class:"muted", title:e.rejection_reason}, "ⓘ") : null
      );
      block.appendChild(row);
    }
    block.appendChild(h("div",{style:{marginTop:".5em"}},
      h("button", {class:"sec", onclick:()=>entryDialog({entry_date: ds})}, `+ ${t("Add entry")}`)
    ));
    wrap.appendChild(block);
  }

  const drafts = entries.filter(e => e.status === "draft");
  if (drafts.length){
    wrap.appendChild(h("div", {style:{marginTop:"1em"}},
      h("button", {class:"success", onclick:async ()=>{
        await api("/time-entries/submit", {method:"POST", body:{ids: drafts.map(x=>x.id)}});
        toast(t("Week submitted."), "ok"); render();
      }}, t("Submit week ({count})", {count: drafts.length}))));
  }
  return wrap;
});

async function entryDialog(template){
  const dlg = h("dialog", {});
  const isNew = !template.id;
  dlg.appendChild(h("header", {}, t(isNew ? "Add entry" : "Edit entry")));
  const body = h("div", {class:"inner"});
  const dateIn = h("input", {type:"date", value: template.entry_date || isoDate(new Date()), required:"1", max: isoDate(new Date())});
  const startIn = h("input", {type:"time", value: template.start_time?.slice(0,5)||"08:00", required:"1"});
  const endIn = h("input", {type:"time", value: template.end_time?.slice(0,5)||"12:00", required:"1"});
  const catSel = h("select", {required:"1"});
  for (const c of CATEGORIES) catSel.appendChild(h("option", {value:c.id, selected: template.category_id===c.id?"1":null}, c.name));
  const com = h("textarea", {rows:"2"}); com.value = template.comment || "";
  body.appendChild(field(t("Date"), dateIn));
  body.appendChild(h("div",{class:"grid grid-2"}, field(t("Start"), startIn), field(t("End"), endIn)));
  body.appendChild(field(t("Category"), catSel));
  body.appendChild(field(t("Comment (optional)"), com));
  dlg.appendChild(body);
  const err = h("div", {class:"error", style:{padding:"0 1.5em"}});
  dlg.appendChild(err);
  dlg.appendChild(h("footer", {},
    h("button", {class:"sec", onclick:()=>{dlg.close(); dlg.remove();}}, t("Cancel")),
    h("button", {onclick: async ()=>{
      try {
        const body = { entry_date: dateIn.value, start_time: startIn.value, end_time: endIn.value, category_id: Number(catSel.value), comment: com.value || null };
        if (isNew) await api("/time-entries", {method:"POST", body});
        else await api("/time-entries/"+template.id, {method:"PUT", body});
        dlg.close(); dlg.remove(); render();
      } catch(e){ err.textContent = errorMessage(e); }
    }}, t("Save"))
  ));
  document.body.appendChild(dlg); dlg.showModal();
}

async function changeRequestDialog(entry){
  const dlg = h("dialog", {});
  dlg.appendChild(h("header", {}, t("Request change")));
  const body = h("div", {class:"inner"});
  body.appendChild(h("p", {class:"muted"}, t("Original: {date} {start}-{end}", {date: fmtDate(entry.entry_date), start: entry.start_time.slice(0,5), end: entry.end_time.slice(0,5)})));
  const dateIn = h("input", {type:"date", value: entry.entry_date});
  const startIn = h("input", {type:"time", value: entry.start_time.slice(0,5)});
  const endIn = h("input", {type:"time", value: entry.end_time.slice(0,5)});
  const catSel = h("select", {});
  for (const c of CATEGORIES) catSel.appendChild(h("option", {value:c.id, selected: entry.category_id===c.id?"1":null}, c.name));
  const reason = h("textarea", {rows:"3", required:"1", placeholder:t("Why is the change needed?")});
  body.appendChild(field(t("Date"), dateIn));
  body.appendChild(h("div",{class:"grid grid-2"}, field(t("Start"), startIn), field(t("End"), endIn)));
  body.appendChild(field(t("Category"), catSel));
  body.appendChild(field(t("Reason"), reason));
  dlg.appendChild(body);
  dlg.appendChild(h("footer", {},
    h("button", {class:"sec", onclick:()=>{dlg.close(); dlg.remove();}}, t("Cancel")),
    h("button", {onclick:async ()=>{
      try {
        await api("/change-requests", {method:"POST", body:{
          time_entry_id: entry.id, new_date: dateIn.value, new_start_time: startIn.value, new_end_time: endIn.value, new_category_id: Number(catSel.value), reason: reason.value
        }});
        toast(t("Change request submitted."), "ok"); dlg.close(); dlg.remove();
      } catch(e){ toast(errorMessage(e), "error"); }
    }}, t("Submit request"))
  ));
  document.body.appendChild(dlg); dlg.showModal();
}

// --- Absences ---
route("/absences", async () => {
  const year = new Date().getFullYear();
  const [absences, balance] = await Promise.all([
    api(`/absences?year=${year}`),
    api(`/leave-balance/${CURRENT_USER.id}?year=${year}`)
  ]);
  const wrap = h("div");
  wrap.appendChild(h("h1", {}, t("Absences")));
  wrap.appendChild(h("div",{class:"kpi"},
    h("div",{class:"box"}, h("div",{class:"label"},t("Annual entitlement")), h("div",{class:"val"}, balance.annual_entitlement + " d")),
    h("div",{class:"box"}, h("div",{class:"label"},t("Already taken")), h("div",{class:"val"}, balance.already_taken + " d")),
    h("div",{class:"box"}, h("div",{class:"label"},t("Approved upcoming")), h("div",{class:"val"}, balance.approved_upcoming + " d")),
    h("div",{class:"box"}, h("div",{class:"label"},t("Requested")), h("div",{class:"val"}, balance.requested + " d")),
    h("div",{class:"box"}, h("div",{class:"label"},t("Available")), h("div",{class:"val", style:{color: balance.available<0?"var(--danger)":"var(--success)"}}, balance.available + " d"))
  ));
  wrap.appendChild(h("div",{class:"row",style:{marginBottom:"1em"}},
    h("button", {onclick:()=>absenceDialog("vacation")}, t("Request vacation")),
    h("button", {class:"danger", onclick:()=>absenceDialog("sick")}, t("Report sick")),
    h("button", {class:"sec", onclick:()=>absenceDialog("training")}, t("Training")),
    h("button", {class:"sec", onclick:()=>absenceDialog("special_leave")}, t("Special leave")),
    h("button", {class:"sec", onclick:()=>absenceDialog("unpaid")}, t("Unpaid")),
    h("button", {class:"sec", onclick:()=>absenceDialog("general_absence")}, t("General absence")),
  ));
  const tab = h("table", {class:"tbl"});
  tab.appendChild(h("thead",{},h("tr",{}, ...["Type","From","To","Status","Comment","Action"].map(label=>h("th",{},t(label))))));
  const tb = h("tbody");
  for (const a of absences){
    tb.appendChild(h("tr",{},
      h("td",{"data-label":t("Type")}, absenceKindLabel(a.kind) + (a.half_day?" (½)":"")),
      h("td",{"data-label":t("From")}, fmtDate(a.start_date)),
      h("td",{"data-label":t("To")}, fmtDate(a.end_date)),
      h("td",{"data-label":t("Status")}, h("span", {class:"chip "+a.status}, statusLabel(a.status))),
      h("td",{"data-label":t("Comment")}, a.comment || ""),
      h("td",{"data-label":t("Action")},
        a.status === "requested" ? h("button", {class:"sec", onclick:async ()=>{
          if (!await confirmDialog("Cancel?","Cancel this request?")) return;
          await api("/absences/"+a.id, {method:"DELETE"}); render();
        }}, t("Cancel")) : null,
        (a.kind === "sick" && a.status === "approved") || a.status === "requested" ? h("button",{class:"sec",style:{marginLeft:".3em"},onclick:()=>absenceEditDialog(a)},t("Edit")) : null
      )
    ));
  }
  tab.appendChild(tb);
  wrap.appendChild(tab);
  return wrap;
});

async function absenceDialog(kind){
  const dlg = h("dialog", {});
  const titleMap = {vacation:"Request vacation",sick:"Report sick",training:"Request training",special_leave:"Request special leave",unpaid:"Request unpaid leave",general_absence:"Request general absence"};
  dlg.appendChild(h("header", {}, t(titleMap[kind] || absenceKindLabel(kind))));
  const body = h("div", {class:"inner"});
  const today = isoDate(new Date());
  const from = h("input", {type:"date", value: today, required:"1"});
  const to = h("input", {type:"date", value: today, required:"1"});
  const half = h("input", {type:"checkbox"});
  const com = h("textarea", {rows:"2"});
  body.appendChild(h("div",{class:"grid grid-2"}, field(t("From"), from), field(t("To"), to)));
  if (kind === "vacation") body.appendChild(field(t("Half day"), half));
  body.appendChild(field(t("Comment (optional)"), com));
  dlg.appendChild(body);
  const err = h("div",{class:"error",style:{padding:"0 1.5em"}});
  dlg.appendChild(err);
  dlg.appendChild(h("footer",{},
    h("button",{class:"sec",onclick:()=>{dlg.close();dlg.remove();}},t("Cancel")),
    h("button",{onclick:async ()=>{
      try {
        await api("/absences",{method:"POST",body:{kind, start_date: from.value, end_date: to.value, half_day: half.checked, comment: com.value||null}});
        toast(t(kind === "sick" ? "Sick leave saved." : "Request submitted."), "ok");
        dlg.close(); dlg.remove(); render();
      } catch(e){ err.textContent = errorMessage(e); }
    }},t("Submit"))
  ));
  document.body.appendChild(dlg); dlg.showModal();
}

async function absenceEditDialog(a){
  const dlg = h("dialog",{});
  dlg.appendChild(h("header",{},t("Edit absence")));
  const body = h("div",{class:"inner"});
  const from = h("input",{type:"date",value:a.start_date,required:"1"});
  const to = h("input",{type:"date",value:a.end_date,required:"1"});
  const com = h("textarea",{rows:"2"}); com.value = a.comment||"";
  body.appendChild(h("div",{class:"grid grid-2"}, field(t("From"), from), field(t("To"), to)));
  body.appendChild(field(t("Comment"), com));
  dlg.appendChild(body);
  const err = h("div",{class:"error",style:{padding:"0 1.5em"}});
  dlg.appendChild(err);
  dlg.appendChild(h("footer",{},
    h("button",{class:"sec",onclick:()=>{dlg.close();dlg.remove();}},t("Cancel")),
    h("button",{onclick:async ()=>{
      try {
        await api("/absences/"+a.id,{method:"PUT",body:{kind:a.kind,start_date:from.value,end_date:to.value,half_day:a.half_day,comment:com.value||null}});
        dlg.close();dlg.remove();render();
      } catch(e){ err.textContent = errorMessage(e); }
    }},t("Save"))
  ));
  document.body.appendChild(dlg); dlg.showModal();
}

// --- Calendar ---
route("/calendar", async () => {
  const params = new URLSearchParams(location.search);
  const today = new Date();
  const year = Number(params.get("year")) || today.getFullYear();
  const month = Number(params.get("month")) || (today.getMonth()+1);
  const monthStr = `${year}-${String(month).padStart(2,"0")}`;
  const entries = await api(`/absences/calendar?month=${monthStr}`);
  const holidays = await api(`/holidays?year=${year}`);
  const hMap = new Map(holidays.map(f => [f.holiday_date, f.name]));

  const wrap = h("div");
  wrap.appendChild(h("h1",{},t("Absence calendar")));
  const next = (month === 12) ? `?year=${year+1}&month=1` : `?year=${year}&month=${month+1}`;
  const prev = (month === 1) ? `?year=${year-1}&month=12` : `?year=${year}&month=${month-1}`;
  wrap.appendChild(h("div",{class:"row",style:{marginBottom:"1em"}},
    h("button",{class:"sec", onclick:()=>go("/calendar"+prev)},`← ${t("Previous month")}`),
    h("strong",{}, fmtMonthYear(new Date(year, month-1, 1))),
    h("button",{class:"sec", onclick:()=>go("/calendar"+next)},`${t("Next month")} →`)
  ));

  const first = new Date(year, month-1, 1);
  const start = monday(first);
  const grid = h("div", {class:"cal"});
  for (const wd of weekdayLabels()) grid.appendChild(h("div",{class:"head"}, wd));
  for (let i=0; i<42; i++){
    const d = addDays(start, i);
    const ds = isoDate(d);
    const other = d.getMonth() !== month-1;
    const hol = hMap.get(ds);
    const day = h("div", {class:"day"+(other?" other":"")+(hol?" h":""), title: hol || ""},
      h("span", {class:"nr"}, d.getDate())
    );
    if (hol) day.appendChild(h("div",{style:{fontSize:"0.7em",color:"#92400E"}}, hol));
    for (const e of entries){
      if (ds >= e.start_date && ds <= e.end_date){
        day.appendChild(h("span",{class:"abs-bar abs-"+e.kind, title: e.name + " · " + absenceKindLabel(e.kind) + (e.comment ? " · " + e.comment : "")}, e.name + (e.half_day?" ½":"")));
      }
    }
    grid.appendChild(day);
    if (i >= 34 && d.getMonth() !== month-1 && (i+1)%7===0) break;
  }
  wrap.appendChild(grid);
  wrap.appendChild(h("div",{class:"row",style:{marginTop:"1em",fontSize:".9em"}},
    ...["vacation","sick","training","special_leave","unpaid","general_absence"].map(kind =>
      h("span",{class:"abs-bar abs-"+kind, style:{padding:".2em .6em"}}, absenceKindLabel(kind)))
  ));
  return wrap;
});

// --- Account ---
route("/account", async () => {
  const wrap = h("div");
  const u = CURRENT_USER;
  wrap.appendChild(h("h1",{},t("My account")));
  if (u.must_change_password){
    wrap.appendChild(h("div",{class:"card",style:{borderColor:"var(--warn)"}},
      h("strong",{},t("Please change your password.")), h("p",{class:"muted"},t("You are using a temporary password."))
    ));
  }
  wrap.appendChild(h("div",{class:"card"},
    h("h2",{},t("Personal data")),
    h("div",{class:"grid grid-2"},
      h("div",{}, h("strong",{},t("Name:" ) + " "), u.first_name + " " + u.last_name),
      h("div",{}, h("strong",{},t("Email:") + " "), u.email),
      h("div",{}, h("strong",{},t("Role:") + " "), roleLabel(u.role)),
      h("div",{}, h("strong",{},t("Weekly hours:") + " "), u.weekly_hours),
      h("div",{}, h("strong",{},t("Annual leave:") + " "), u.annual_leave_days),
      h("div",{}, h("strong",{},t("Start date:") + " "), fmtDate(u.start_date)),
    )
  ));

  const card = h("div",{class:"card"});
  card.appendChild(h("h2",{},t("Change password")));
  const cur = h("input",{type:"password",autocomplete:"current-password"});
  const nw = h("input",{type:"password",autocomplete:"new-password",minlength:"12"});
  if (!u.must_change_password) card.appendChild(field(t("Current password"), cur));
  card.appendChild(field(t("New password (min 12 chars)"), nw));
  const err = h("div",{class:"error"});
  card.appendChild(err);
  card.appendChild(h("button",{onclick:async ()=>{
    err.textContent = "";
    try {
      await api("/auth/password",{method:"PUT",body:{current_password: u.must_change_password?null:cur.value, new_password: nw.value}});
      CURRENT_USER.must_change_password = false;
      toast(t("Password changed."), "ok");
      cur.value = ""; nw.value = "";
    } catch(e){ err.textContent = errorMessage(e); }
  }},t("Save")));
  wrap.appendChild(card);

  const ot = await api(`/reports/overtime?year=${new Date().getFullYear()}`);
  const cum = ot.reduce((s,m)=> s + m.diff_min, 0);
  const otCard = h("div",{class:"card"});
  otCard.appendChild(h("h2",{},t("Overtime balance {year}", {year: new Date().getFullYear()})));
  otCard.appendChild(h("div",{class:"kpi"}, h("div",{class:"box"}, h("div",{class:"label"},t("Balance")), h("div",{class:"val", style:{color: cum<0?"var(--danger)":"var(--success)"}}, minToHM(cum)))));
  const tab = h("table",{class:"tbl"});
  tab.appendChild(h("thead",{},h("tr",{},...["Month","Target","Actual","Diff","Cumulative"].map(label=>h("th",{},t(label))))));
  const tb = h("tbody");
  for (const m of ot){
    if (m.target_min === 0 && m.actual_min === 0) continue;
    tb.appendChild(h("tr",{},
      h("td",{"data-label":t("Month")}, m.month),
      h("td",{"data-label":t("Target")}, minToHM(m.target_min)),
      h("td",{"data-label":t("Actual")}, minToHM(m.actual_min)),
      h("td",{"data-label":t("Diff")}, minToHM(m.diff_min)),
      h("td",{"data-label":t("Cumulative")}, minToHM(m.cumulative_min))
    ));
  }
  tab.appendChild(tb);
  otCard.appendChild(tab);
  wrap.appendChild(otCard);
  return wrap;
});

// --- Dashboard ---
route("/dashboard", async () => {
  if (CURRENT_USER.role === "employee"){ go("/time", false); return h("div"); }
  const [te, ab, cr] = await Promise.all([
    api("/time-entries/all?status=submitted"),
    api("/absences/all?status=requested"),
    api("/change-requests/all"),
  ]);
  const users = await api("/users");
  const uMap = new Map(users.map(u => [u.id, u]));

  const wrap = h("div");
  wrap.appendChild(h("h1",{},t("Dashboard")));
  wrap.appendChild(h("div",{class:"kpi"},
    h("div",{class:"box"}, h("div",{class:"label"},t("Submitted entries")), h("div",{class:"val"}, te.length)),
    h("div",{class:"box"}, h("div",{class:"label"},t("Open requests")), h("div",{class:"val"}, ab.length)),
    h("div",{class:"box"}, h("div",{class:"label"},t("Change requests")), h("div",{class:"val"}, cr.length)),
  ));

  const teCard = h("div",{class:"card"});
  teCard.appendChild(h("h2",{},t("Submitted time entries")));
  if (te.length === 0) teCard.appendChild(h("p",{class:"muted"},t("No open entries.")));
  else {
    const groups = {};
    for (const z of te){
      const k = z.user_id + "|" + isoWeek(new Date(z.entry_date)) + "|" + new Date(z.entry_date).getFullYear();
      (groups[k] = groups[k] || []).push(z);
    }
    for (const [key, items] of Object.entries(groups)){
      const [uid, kw] = key.split("|");
      const u = uMap.get(Number(uid));
      const total = items.reduce((s,e)=>s+durMin(e.start_time.slice(0,5),e.end_time.slice(0,5)),0);
      const blk = h("div",{class:"dayblock"});
      blk.appendChild(h("div",{class:"row"},
        h("strong",{style:{flex:"1"}}, `${u.first_name} ${u.last_name} - ${t("Week {week}", {week: kw})}`),
        h("span",{}, "Σ " + minToHM(total)),
        h("button",{class:"success", onclick:async ()=>{
          await api("/time-entries/batch-approve",{method:"POST",body:{ids: items.map(x=>x.id)}});
          toast(t("Approved."),"ok"); render();
        }}, t("Approve all"))
      ));
      const tab = h("table",{class:"tbl"});
      for (const z of items){
        const c = CATEGORIES.find(x=>x.id===z.category_id) || {name:"?",color:"#999"};
        tab.appendChild(h("tr",{},
          h("td",{"data-label":t("Date")}, fmtDate(z.entry_date)),
          h("td",{"data-label":t("Time")}, z.start_time.slice(0,5)+"–"+z.end_time.slice(0,5)),
          h("td",{"data-label":t("Category")}, h("span",{class:"cat-bar",style:{background:c.color}}), c.name),
          h("td",{"data-label":t("Comment")}, z.comment||""),
          h("td",{"data-label":t("Action")},
            h("button",{class:"success",onclick:async ()=>{await api("/time-entries/"+z.id+"/approve",{method:"POST"}); render();}},"✓"),
            h("button",{class:"danger", style:{marginLeft:".3em"}, onclick:async ()=>{
              const r = await confirmDialog("Reject?","",{reason:true,danger:true,confirm:"Reject"});
              if (r === null) return;
              await api("/time-entries/"+z.id+"/reject",{method:"POST",body:{reason:r}}); render();
            }},"✗")
          )
        ));
      }
      blk.appendChild(tab);
      teCard.appendChild(blk);
    }
  }
  wrap.appendChild(teCard);

  const abCard = h("div",{class:"card"});
  abCard.appendChild(h("h2",{},t("Open absence requests")));
  if (ab.length === 0) abCard.appendChild(h("p",{class:"muted"},t("No open requests.")));
  else {
    const tab = h("table",{class:"tbl"});
    tab.appendChild(h("thead",{},h("tr",{},...["Employee","Type","From","To","Comment","Action"].map(label=>h("th",{},t(label))))));
    for (const a of ab){
      const u = uMap.get(a.user_id);
      tab.appendChild(h("tr",{},
        h("td",{"data-label":t("Employee")}, u ? u.first_name+" "+u.last_name : "?"),
        h("td",{"data-label":t("Type")}, absenceKindLabel(a.kind) + (a.half_day?" (½)":"")),
        h("td",{"data-label":t("From")}, fmtDate(a.start_date)),
        h("td",{"data-label":t("To")}, fmtDate(a.end_date)),
        h("td",{"data-label":t("Comment")}, a.comment||""),
        h("td",{"data-label":t("Action")},
          h("button",{class:"success",onclick:async ()=>{await api("/absences/"+a.id+"/approve",{method:"POST"}); render();}},t("Approve")),
          h("button",{class:"danger",style:{marginLeft:".3em"},onclick:async ()=>{
            const r = await confirmDialog("Reject?","",{reason:true,danger:true,confirm:"Reject"});
            if (r===null) return;
            await api("/absences/"+a.id+"/reject",{method:"POST",body:{reason:r}}); render();
          }},t("Reject"))
        )
      ));
    }
    abCard.appendChild(tab);
  }
  wrap.appendChild(abCard);

  const crCard = h("div",{class:"card"});
  crCard.appendChild(h("h2",{},t("Change requests")));
  if (cr.length === 0) crCard.appendChild(h("p",{class:"muted"},t("No open change requests.")));
  else {
    for (const a of cr){
      const u = uMap.get(a.user_id);
      const blk = h("div",{class:"dayblock"});
      blk.appendChild(h("strong",{},(u?u.first_name+" "+u.last_name:"?")+" - "+t("Change request")));
      blk.appendChild(h("p",{},t("Reason: {reason}", {reason: a.reason})));
      blk.appendChild(h("p",{class:"muted"}, t("New values: {date} {start}-{end}", {date: a.new_date||"-", start: a.new_start_time||"", end: a.new_end_time||""})));
      blk.appendChild(h("div",{class:"row"},
        h("button",{class:"success",onclick:async ()=>{await api("/change-requests/"+a.id+"/approve",{method:"POST"}); render();}},t("Approve & apply")),
        h("button",{class:"danger",onclick:async ()=>{
          const r = await confirmDialog("Reject?","",{reason:true,danger:true,confirm:"Reject"});
          if (r===null) return;
          await api("/change-requests/"+a.id+"/reject",{method:"POST",body:{reason:r}}); render();
        }},t("Reject"))
      ));
      crCard.appendChild(blk);
    }
  }
  wrap.appendChild(crCard);
  return wrap;
});

// --- Reports ---
route("/reports", async () => {
  const wrap = h("div");
  wrap.appendChild(h("h1",{},t("Reports")));
  const today = new Date();
  const month = `${today.getFullYear()}-${String(today.getMonth()+1).padStart(2,"0")}`;

  const users = CURRENT_USER.role === "employee" ? [CURRENT_USER] : await api("/users");
  const filt = h("div",{class:"card"});
  filt.appendChild(h("h2",{},t("Monthly report")));
  const userSel = h("select",{});
  for (const u of users) userSel.appendChild(h("option",{value:u.id, selected: u.id===CURRENT_USER.id?"1":null}, u.first_name+" "+u.last_name));
  const monthIn = h("input",{type:"month",value:month});
  filt.appendChild(h("div",{class:"grid grid-2"}, field(t("Employee"), userSel), field(t("Month"), monthIn)));
  const out = h("div");
  filt.appendChild(h("div",{class:"row"},
    h("button",{onclick:async ()=>{
      const r = await api(`/reports/month?user_id=${userSel.value}&month=${monthIn.value}`);
      out.innerHTML = "";
      out.appendChild(h("div",{class:"kpi"},
        h("div",{class:"box"}, h("div",{class:"label"},t("Target")), h("div",{class:"val"},minToHM(r.target_min))),
        h("div",{class:"box"}, h("div",{class:"label"},t("Actual")), h("div",{class:"val"},minToHM(r.actual_min))),
        h("div",{class:"box"}, h("div",{class:"label"},t("Diff")), h("div",{class:"val"},minToHM(r.diff_min))),
      ));
      const tab = h("table",{class:"tbl"});
      tab.appendChild(h("thead",{},h("tr",{},...["Date","Weekday","Entries","Actual","Target","Note"].map(label=>h("th",{},t(label))))));
      for (const day of r.days){
        tab.appendChild(h("tr",{},
          h("td",{"data-label":t("Date")}, day.date),
          h("td",{"data-label":t("Weekday")}, t(day.weekday)),
          h("td",{"data-label":t("Entries")}, day.entries.map(e => `${e.start_time.slice(0,5)}–${e.end_time.slice(0,5)} ${e.category}`).join("; ")),
          h("td",{"data-label":t("Actual")}, minToHM(day.actual_min)),
          h("td",{"data-label":t("Target")}, minToHM(day.target_min)),
          h("td",{"data-label":t("Note")}, day.holiday || (day.absence ? absenceKindLabel(day.absence) : ""))
        ));
      }
      out.appendChild(tab);
      const cs = h("div",{},h("h3",{},t("By category")));
      for (const [k,m] of Object.entries(r.category_totals)) cs.appendChild(h("div",{},`${k}: ${minToHM(m)}`));
      out.appendChild(cs);
    }},t("Show")),
    h("a",{class:"btn sec",href:"#",onclick:e=>{e.preventDefault(); window.open(`/api/v1/reports/month/csv?user_id=${userSel.value}&month=${monthIn.value}`);}},t("Export CSV"))
  ));
  filt.appendChild(out);
  wrap.appendChild(filt);

  if (CURRENT_USER.role !== "employee"){
    const team = h("div",{class:"card"});
    team.appendChild(h("h2",{},t("Team report")));
    const mIn = h("input",{type:"month",value:month});
    const tOut = h("div");
    team.appendChild(h("div",{class:"row"}, field(t("Month"), mIn),
      h("button",{onclick:async ()=>{
        const r = await api(`/reports/team?month=${mIn.value}`);
        tOut.innerHTML = "";
        const tab = h("table",{class:"tbl"});
        tab.appendChild(h("thead",{},h("tr",{},...["Employee","Target","Actual","Diff","Vacation","Sick"].map(label=>h("th",{},t(label))))));
        for (const z of r){
          tab.appendChild(h("tr",{},
            h("td",{"data-label":t("Employee")}, z.name),
            h("td",{"data-label":t("Target")}, minToHM(z.target_min)),
            h("td",{"data-label":t("Actual")}, minToHM(z.actual_min)),
            h("td",{"data-label":t("Diff")}, minToHM(z.diff_min)),
            h("td",{"data-label":t("Vacation")}, z.vacation_days),
            h("td",{"data-label":t("Sick")}, z.sick_days),
          ));
        }
        tOut.appendChild(tab);
      }},t("Show"))
    ));
    team.appendChild(tOut);
    wrap.appendChild(team);
  }

  const cat = h("div",{class:"card"});
  cat.appendChild(h("h2",{},t("Category breakdown")));
  const from = h("input",{type:"date",value: isoDate(new Date(today.getFullYear(),0,1))});
  const to = h("input",{type:"date",value: isoDate(today)});
  const cOut = h("div");
  cat.appendChild(h("div",{class:"grid grid-2"}, field(t("From"), from), field(t("To"), to)));
  cat.appendChild(h("button",{onclick:async ()=>{
    const r = await api(`/reports/categories?from=${from.value}&to=${to.value}${CURRENT_USER.role==="employee"?"&user_id="+CURRENT_USER.id:""}`);
    cOut.innerHTML = "";
    const total = r.reduce((s,x)=>s+x.minutes,0);
    if (!total){ cOut.appendChild(h("p",{class:"muted"},t("No data."))); return; }
    const svg = document.createElementNS("http://www.w3.org/2000/svg","svg");
    svg.setAttribute("viewBox","0 0 200 200"); svg.setAttribute("width","240"); svg.setAttribute("height","240");
    let acc = 0;
    for (const x of r){
      const a0 = acc/total*Math.PI*2 - Math.PI/2;
      acc += x.minutes;
      const a1 = acc/total*Math.PI*2 - Math.PI/2;
      const large = (a1-a0) > Math.PI ? 1 : 0;
      const x0 = 100+90*Math.cos(a0), y0 = 100+90*Math.sin(a0);
      const x1 = 100+90*Math.cos(a1), y1 = 100+90*Math.sin(a1);
      const path = document.createElementNS("http://www.w3.org/2000/svg","path");
      path.setAttribute("d", `M100,100 L${x0},${y0} A90,90 0 ${large},1 ${x1},${y1} Z`);
      path.setAttribute("fill", x.color);
      svg.appendChild(path);
    }
    const wr = h("div",{class:"row",style:{alignItems:"flex-start",gap:"2em"}});
    wr.appendChild(svg);
    const list = h("div");
    for (const x of r) list.appendChild(h("div",{},h("span",{class:"cat-bar",style:{background:x.color}}), `${x.category}: ${minToHM(x.minutes)} (${(x.minutes/total*100).toFixed(1)}%)`));
    wr.appendChild(list);
    cOut.appendChild(wr);
  }},t("Run")));
  cat.appendChild(cOut);
  wrap.appendChild(cat);
  return wrap;
});

// --- Admin ---
function adminTabs(active){
  const tabs = [["users","Users"],["categories","Categories"],["holidays","Holidays"],["audit-log","Audit log"],["settings","Settings"]];
  return h("div",{class:"row",style:{marginBottom:"1em"}},
    ...tabs.map(([k,n])=>h("a",{href:"/admin/"+k,"data-link":"1",class:"btn "+(active===k?"":"sec")},t(n)))
  );
}

route("/admin", async () => { history.replaceState({},"","/admin/users"); return await routes.find(r=>r.pattern==="/admin/users").handler({}); });

route("/admin/users", async () => {
  if (CURRENT_USER.role !== "admin") return notAllowed();
  const wrap = h("div"); wrap.appendChild(adminTabs("users"));
  const list = await api("/users");
  wrap.appendChild(h("button",{style:{marginBottom:"1em"},onclick:()=>userDialog()},`+ ${t("New user")}`));
  const tab = h("table",{class:"tbl"});
  tab.appendChild(h("thead",{},h("tr",{},...["Name","Email","Role","Hours","Leave","Active","Action"].map(label=>h("th",{},t(label))))));
  for (const u of list){
    tab.appendChild(h("tr",{},
      h("td",{"data-label":t("Name")}, u.first_name+" "+u.last_name),
      h("td",{"data-label":t("Email")}, u.email),
      h("td",{"data-label":t("Role")}, roleLabel(u.role)),
      h("td",{"data-label":t("Hours")}, u.weekly_hours),
      h("td",{"data-label":t("Leave")}, u.annual_leave_days),
      h("td",{"data-label":t("Active")}, u.active?t("Yes"):t("No")),
      h("td",{"data-label":t("Action")},
        h("button",{class:"sec",onclick:()=>userDialog(u)},t("Edit")),
        h("button",{class:"sec",style:{marginLeft:".3em"},onclick:async ()=>{
          if (!await confirmDialog("Reset password?","A temporary password will be generated.")) return;
          const r = await api("/users/"+u.id+"/reset-password",{method:"POST"});
          alert(t("Temporary password: {password}", {password: r.temporary_password}));
        }},t("Reset PW")),
        u.active ? h("button",{class:"danger",style:{marginLeft:".3em"},onclick:async ()=>{
          if (!await confirmDialog("Deactivate?","",{danger:true,confirm:"Deactivate"})) return;
          await api("/users/"+u.id+"/deactivate",{method:"POST"}); render();
        }},t("Deactivate")) : null
      )
    ));
  }
  wrap.appendChild(tab);
  return wrap;
});

async function userDialog(u){
  const isNew = !u;
  const dlg = h("dialog",{});
  dlg.appendChild(h("header",{}, t(isNew?"New user":"Edit user")));
  const body = h("div",{class:"inner"});
  const email = h("input",{type:"email",value:u?.email||"",required:"1"});
  const fn = h("input",{value:u?.first_name||"",required:"1"});
  const ln = h("input",{value:u?.last_name||"",required:"1"});
  const role = h("select",{}, ...["employee","team_lead","admin"].map(value=>h("option",{value,selected:u?.role===value?"1":null},roleLabel(value))));
  const wh = h("input",{type:"number",step:"0.5",value:u?.weekly_hours||39,required:"1"});
  const ld = h("input",{type:"number",value:u?.annual_leave_days||30,required:"1"});
  const sd = h("input",{type:"date",value:u?.start_date||isoDate(new Date()),required:"1"});
  body.appendChild(field(t("Email"), email));
  body.appendChild(h("div",{class:"grid grid-2"}, field(t("First name"), fn), field(t("Last name"), ln)));
  body.appendChild(field(t("Role"), role));
  body.appendChild(h("div",{class:"grid grid-3"}, field(t("Weekly hours"), wh), field(t("Annual leave days"), ld), field(t("Start date"), sd)));
  let activeChk;
  if (!isNew) {
    activeChk = h("input",{type:"checkbox"}); activeChk.checked = u.active;
    body.appendChild(field(t("Active"), activeChk));
  }
  dlg.appendChild(body);
  const err = h("div",{class:"error",style:{padding:"0 1.5em"}});
  dlg.appendChild(err);
  dlg.appendChild(h("footer",{},
    h("button",{class:"sec",onclick:()=>{dlg.close();dlg.remove();}},t("Cancel")),
    h("button",{onclick:async ()=>{
      try{
        const body = {email: email.value, first_name: fn.value, last_name: ln.value, role: role.value, weekly_hours: Number(wh.value), annual_leave_days: Number(ld.value), start_date: sd.value};
        if (isNew){
          const r = await api("/users",{method:"POST",body});
          if (r.temporary_password) alert(t("User created. Temporary password: {password}", {password: r.temporary_password}));
        } else {
          body.active = activeChk.checked;
          await api("/users/"+u.id,{method:"PUT",body});
        }
        dlg.close();dlg.remove();render();
      }catch(e){err.textContent = errorMessage(e);}
    }},t("Save"))
  ));
  document.body.appendChild(dlg); dlg.showModal();
}

route("/admin/categories", async () => {
  if (CURRENT_USER.role !== "admin") return notAllowed();
  const wrap = h("div"); wrap.appendChild(adminTabs("categories"));
  const r = await api("/categories");
  wrap.appendChild(h("button",{style:{marginBottom:"1em"},onclick:()=>categoryDialog()},`+ ${t("New category")}`));
  const tab = h("table",{class:"tbl"});
  tab.appendChild(h("thead",{},h("tr",{},...["Color","Name","Description","Order","Action"].map(label=>h("th",{},t(label))))));
  for (const c of r){
    tab.appendChild(h("tr",{},
      h("td",{"data-label":t("Color")}, h("span",{class:"cat-bar",style:{background:c.color}}), " ", c.color),
      h("td",{"data-label":t("Name")}, c.name),
      h("td",{"data-label":t("Description")}, c.description||""),
      h("td",{"data-label":t("Order")}, c.sort_order),
      h("td",{"data-label":t("Action")}, h("button",{class:"sec",onclick:()=>categoryDialog(c)},t("Edit")))
    ));
  }
  wrap.appendChild(tab); return wrap;
});

async function categoryDialog(c){
  const dlg = h("dialog",{});
  dlg.appendChild(h("header",{}, t(c?"Edit category":"New category")));
  const body = h("div",{class:"inner"});
  const name = h("input",{value:c?.name||"",required:"1"});
  const desc = h("input",{value:c?.description||""});
  const color = h("input",{type:"color",value:c?.color||"#2563EB"});
  const sort = h("input",{type:"number",value:c?.sort_order||0});
  body.appendChild(field(t("Name"), name));
  body.appendChild(field(t("Description"), desc));
  body.appendChild(h("div",{class:"grid grid-2"}, field(t("Color"), color), field(t("Order"), sort)));
  dlg.appendChild(body);
  dlg.appendChild(h("footer",{},
    h("button",{class:"sec",onclick:()=>{dlg.close();dlg.remove();}},t("Cancel")),
    h("button",{onclick:async ()=>{
      const body = {name:name.value, description:desc.value||null, color:color.value, sort_order:Number(sort.value)};
      if (c) await api("/categories/"+c.id,{method:"PUT",body});
      else await api("/categories",{method:"POST",body});
      CATEGORIES = await api("/categories");
      dlg.close();dlg.remove();render();
    }},t("Save"))
  ));
  document.body.appendChild(dlg); dlg.showModal();
}

route("/admin/holidays", async () => {
  if (CURRENT_USER.role !== "admin") return notAllowed();
  const wrap = h("div"); wrap.appendChild(adminTabs("holidays"));
  const year = new Date().getFullYear();
  const r = await api(`/holidays?year=${year}`);
  const dIn = h("input",{type:"date"});
  const nIn = h("input",{placeholder:t("Name")});
  wrap.appendChild(h("div",{class:"card"},
    h("h2",{},t("Add holiday")),
    h("div",{class:"row"}, dIn, nIn,
      h("button",{onclick:async ()=>{
        if(!dIn.value || !nIn.value){toast(t("Date and name required"),"error");return;}
        await api("/holidays",{method:"POST",body:{holiday_date:dIn.value,name:nIn.value}});
        render();
      }},t("Add")))
  ));
  const tab = h("table",{class:"tbl"});
  tab.appendChild(h("thead",{},h("tr",{},...["Date","Name","Action"].map(label=>h("th",{},t(label))))));
  for (const f of r){
    tab.appendChild(h("tr",{},
      h("td",{"data-label":t("Date")}, fmtDate(f.holiday_date)),
      h("td",{"data-label":t("Name")}, f.name),
      h("td",{"data-label":t("Action")}, h("button",{class:"danger",onclick:async ()=>{
        if(!await confirmDialog("Delete?","",{danger:true,confirm:"Delete"}))return;
        await api("/holidays/"+f.id,{method:"DELETE"});render();
      }},t("Delete")))
    ));
  }
  wrap.appendChild(tab); return wrap;
});

route("/admin/audit-log", async () => {
  if (CURRENT_USER.role !== "admin") return notAllowed();
  const wrap = h("div"); wrap.appendChild(adminTabs("audit-log"));
  const r = await api("/audit-log");
  const tab = h("table",{class:"tbl"});
  tab.appendChild(h("thead",{},h("tr",{},...["Time","User","Action","Table","Record"].map(label=>h("th",{},t(label))))));
  for (const e of r){
    tab.appendChild(h("tr",{},
      h("td",{"data-label":t("Time")}, fmtDateTime(e.occurred_at)),
      h("td",{"data-label":t("User")}, e.user_id),
      h("td",{"data-label":t("Action")}, t(e.action)),
      h("td",{"data-label":t("Table")}, t(e.table_name)),
      h("td",{"data-label":t("Record")}, e.record_id)
    ));
  }
  wrap.appendChild(tab); return wrap;
});

route("/admin/settings", async () => {
  if (CURRENT_USER.role !== "admin") return notAllowed();
  const settings = await api("/settings");
  const wrap = h("div"); wrap.appendChild(adminTabs("settings"));
  const card = h("div", {class:"card"});
  const language = h(
    "select",
    {},
    ...Object.entries(LANGUAGES).map(([code, meta]) =>
      h("option", {value:code, selected:settings.ui_language===code?"1":null}, meta.label)
    )
  );
  const err = h("div", {class:"error"});
  card.appendChild(h("h2", {}, t("Language settings")));
  card.appendChild(field(t("Interface language"), language));
  card.appendChild(h("p", {class:"muted"}, t("Missing translations fall back to English.")));
  card.appendChild(err);
  card.appendChild(h("button", {onclick:async () => {
    err.textContent = "";
    try {
      const saved = await api("/settings", {method:"PUT", body:{ui_language: language.value}});
      applySettings(saved);
      SETTINGS_PROMISE = Promise.resolve(APP_SETTINGS);
      toast(t("Language saved."), "ok");
      render();
    } catch (e) {
      err.textContent = errorMessage(e);
    }
  }}, t("Save")));
  wrap.appendChild(card);
  return wrap;
});

render();
