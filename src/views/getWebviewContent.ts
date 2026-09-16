export function getWebviewContent(opts: { nonce: string; cspSource: string }): string {
  const { nonce, cspSource } = opts;

  return `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src ${cspSource} 'nonce-${nonce}'; script-src 'nonce-${nonce}';">
<style nonce="${nonce}">
  * { box-sizing: border-box; }
  html, body { height: 100%; }
  body {
    font-family: var(--vscode-font-family);
    font-size: 13px;
    color: var(--vscode-sideBar-foreground, var(--vscode-editor-foreground));
    background: var(--vscode-sideBar-background);
    margin: 0;
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .card {
    background: var(--vscode-editorWidget-background, var(--vscode-sideBar-background));
    border: 1px solid var(--vscode-widget-border, rgba(128,128,128,.25));
    border-radius: 10px;
    padding: 12px;
  }
  .row { display: flex; align-items: center; gap: 8px; }
  .spread { justify-content: space-between; }
  .grow { flex: 1; min-width: 0; }
  button {
    font-family: inherit; font-size: 12px;
    display: inline-flex; align-items: center; justify-content: center; gap: 6px;
    padding: 7px 12px; border: none; border-radius: 7px; cursor: pointer;
    background: var(--vscode-button-background);
    color: var(--vscode-button-foreground);
  }
  button:hover:not(:disabled) { outline: 1px solid var(--vscode-focusBorder); }
  button:disabled { opacity: .4; cursor: default; }
  button.sec { background: var(--vscode-button-secondaryBackground); color: var(--vscode-button-secondaryForeground); }
  button.icon { padding: 7px 9px; }
  button svg { width: 14px; height: 14px; fill: currentColor; flex: none; }
  .seg { display: flex; border: 1px solid var(--vscode-widget-border, rgba(128,128,128,.25)); border-radius: 8px; overflow: hidden; }
  .seg button { border-radius: 0; background: transparent; color: inherit; padding: 5px 10px; flex: 1; }
  .seg button.on { background: var(--vscode-button-secondaryBackground); color: var(--vscode-button-secondaryForeground); font-weight: 600; }
  .progress { display: flex; gap: 3px; margin: 10px 0 4px; }
  .bar { height: 5px; flex: 1; border-radius: 3px; background: var(--vscode-editorWidget-border, rgba(128,128,128,.25)); cursor: pointer; }
  .bar.done { background: var(--vscode-charts-blue, #38bdf8); opacity: .45; }
  .bar.cur { background: var(--vscode-charts-orange, #f59e0b); opacity: 1; }
  .counter { font-size: 11px; opacity: .8; }
  .summary { font-size: 12px; opacity: .9; margin-top: 6px; line-height: 1.45; }
  .badge {
    display: inline-flex; align-items: center; gap: 4px;
    font-size: 10.5px; padding: 2px 8px; border-radius: 20px;
    background: var(--vscode-badge-background); color: var(--vscode-badge-foreground);
    white-space: nowrap;
  }
  .badge.mono { font-family: var(--vscode-editor-font-family, monospace); }
  .idx {
    font-family: var(--vscode-editor-font-family, monospace); font-size: 11px; font-weight: 600;
    background: var(--vscode-badge-background); color: var(--vscode-badge-foreground);
    min-width: 22px; height: 22px; border-radius: 6px;
    display: inline-flex; align-items: center; justify-content: center; flex: none;
  }
  .title { font-weight: 600; font-size: 13px; line-height: 1.35; }
  .expl { line-height: 1.55; margin-top: 8px; font-size: 12.5px; }
  code {
    font-family: var(--vscode-editor-font-family, monospace); font-size: 11.5px;
    background: var(--vscode-textCodeBlock-background, rgba(128,128,128,.14));
    border-radius: 4px; padding: 1px 5px;
  }
  .transition { display: flex; align-items: stretch; gap: 8px; margin-top: 10px; }
  .state {
    flex: 1; min-width: 0;
    font-family: var(--vscode-editor-font-family, monospace); font-size: 11.5px;
    background: var(--vscode-textCodeBlock-background, rgba(128,128,128,.14));
    border-radius: 7px; padding: 7px 9px;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .state .lbl { display: block; font-size: 9.5px; opacity: .65; letter-spacing: .06em; margin-bottom: 2px; font-family: var(--vscode-font-family); }
  .arrow { align-self: center; opacity: .6; font-size: 14px; flex: none; }
  .warn {
    display: flex; gap: 8px; align-items: flex-start;
    margin-top: 10px; padding: 9px 11px; border-radius: 7px;
    background: rgba(245, 158, 11, .09);
    border-left: 3px solid var(--vscode-editorWarning-foreground, #f59e0b);
    line-height: 1.5;
  }
  .warn svg { width: 14px; height: 14px; fill: var(--vscode-editorWarning-foreground, #f59e0b); flex: none; margin-top: 2px; }
  .steps { margin-top: 4px; }
  .step-row {
    display: flex; align-items: center; gap: 8px;
    padding: 6px 8px; border-radius: 7px; cursor: pointer;
  }
  .step-row:hover { background: var(--vscode-list-hoverBackground); }
  .step-row.cur { background: var(--vscode-list-activeSelectionBackground); color: var(--vscode-list-activeSelectionForeground); }
  .step-row .t { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .step-row svg { width: 13px; height: 13px; fill: currentColor; opacity: .8; flex: none; }
  .dot { width: 7px; height: 7px; border-radius: 50%; background: var(--vscode-editorWarning-foreground, #f59e0b); flex: none; }
  .muted { opacity: .7; font-size: 11px; }
  .logo { display: block; margin: 4px auto 2px; }
  .hero { text-align: center; padding: 18px 10px 8px; }
  .hero h2 { font-weight: 600; font-size: 15px; margin: 10px 0 6px; }
  .hero p { margin: 0; line-height: 1.5; font-size: 12px; opacity: .85; }
  .stack { display: flex; flex-direction: column; gap: 8px; }
  .stack button { width: 100%; padding: 9px 12px; justify-content: flex-start; }
  .kv { margin-top: 8px; font-size: 11.5px; opacity: .85; }
</style>
</head>
<body>
<div id="root"></div>
<script nonce="${nonce}">
(function () {
  var vscode = acquireVsCodeApi();
  var state = { plan: null, current: 0, autoplay: false, style: 'standard' };

  var I = {
    chevL: '<svg viewBox="0 0 16 16"><path d="M10.5 3 6 8l4.5 5 1-1L8 8l3.5-4z"/></svg>',
    chevR: '<svg viewBox="0 0 16 16"><path d="M5.5 3 10 8l-4.5 5-1-1L8 8 4.5 4z"/></svg>',
    play: '<svg viewBox="0 0 16 16"><path d="M5 3.2v9.6c0 .6.7 1 1.2.6l7-4.8c.5-.3.5-1 0-1.2l-7-4.8C5.7 2.2 5 2.6 5 3.2z"/></svg>',
    pause: '<svg viewBox="0 0 16 16"><rect x="4.5" y="3" width="2.6" height="10" rx="1"/><rect x="8.9" y="3" width="2.6" height="10" rx="1"/></svg>',
    shield: '<svg viewBox="0 0 16 16"><path d="M8 1.3 3.2 3.1v3.9c0 3.2 1.9 6 4.8 7.3 2.9-1.3 4.8-4.1 4.8-7.3V3.1L8 1.3zm0 1.6 3.4 1.3v2.8c0 2.4-1.4 4.6-3.4 5.6-2-1-3.4-3.2-3.4-5.6V4.2L8 2.9z"/></svg>',
    zap: '<svg viewBox="0 0 16 16"><path d="M9.5 1 3.5 9H7l-1 6 6.5-8H9l.5-6z"/></svg>',
    diff: '<svg viewBox="0 0 16 16"><path d="M2 3h2.5v1.5H2zM2 7h5.5v1.5H2zM2 11h3.5v1.5H2zM11 1v5h3V1h-3zm1 4h1V2h-1v3zM11 10v5h3v-5h-3zm1 4h1v-3h-1v3z"/></svg>',
    cursor: '<svg viewBox="0 0 16 16"><path d="M5 3 8.3 13l1.2-3.7 3.7-1.2L5 3zm4.9 6.1-.2.6-.6.2 2 3 1-.4-2.2-3.4z"/></svg>',
    exit: '<svg viewBox="0 0 16 16"><path d="M8 8.7 4.9 11.8l-.7-.7L7.3 8 4.2 4.9l.7-.7L8 7.3l3.1-3.1.7.7L8.7 8l3.1 3.1-.7.7L8 8.7z"/></svg>',
    list: '<svg viewBox="0 0 16 16"><path d="M2.5 3.5h11V5h-11zM2.5 7.5h11V9h-11zM2.5 11.5h7V13h-7z"/></svg>'
  };

  function esc(s) {
    return String(s == null ? '' : s).replace(/[&<>"']/g, function (c) {
      return { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c];
    });
  }
  function md(s) {
    return esc(s)
      .replace(/\`([^\`]+)\`/g, '<code>$1</code>')
      .replace(/\\*\\*([^*]+)\\*\\*/g, '<strong>$1</strong>');
  }
  function actionGlyph(a) {
    return { init: I.play, pass: I.chevR, validate: I.shield, mutate: I.zap, commit: I.diff, 'return': I.exit }[a] || I.chevR;
  }

  document.addEventListener('click', function (e) {
    var el = e.target.closest('[data-act]');
    if (!el) return;
    var act = el.getAttribute('data-act');
    if (act === 'next') vscode.postMessage({ t: 'next' });
    else if (act === 'prev') vscode.postMessage({ t: 'prev' });
    else if (act === 'jump') vscode.postMessage({ t: 'jump', i: parseInt(el.getAttribute('data-i'), 10) });
    else if (act === 'autoplay') vscode.postMessage({ t: 'autoplay' });
    else if (act === 'exit') vscode.postMessage({ t: 'exit' });
    else if (act === 'browse') vscode.postMessage({ t: 'browse' });
    else if (act === 'start') vscode.postMessage({ t: 'start', mode: el.getAttribute('data-mode') });
    else if (act === 'style') vscode.postMessage({ t: 'style', s: el.getAttribute('data-s') });
  });
  document.addEventListener('keydown', function (e) {
    if (e.key === 'ArrowRight') vscode.postMessage({ t: 'next' });
    else if (e.key === 'ArrowLeft') vscode.postMessage({ t: 'prev' });
  });
  window.addEventListener('message', function (e) {
    var m = e.data || {};
    if (m.t === 'state') { state.plan = m.plan; state.current = m.current; render(); }
    else if (m.t === 'autoplay') { state.autoplay = m.on; render(); }
    else if (m.t === 'style') { state.style = m.s; render(); }
  });

  function render() {
    document.getElementById('root').innerHTML = state.plan ? renderPlan() : renderWelcome();
  }

  function renderWelcome() {
    var h = '';
    h += '<div class="card hero">';
    h += '<svg class="logo" width="56" height="56" viewBox="0 0 128 128"><rect x="4" y="4" width="120" height="120" rx="26" fill="none" stroke="currentColor" stroke-width="6" opacity=".35"/><rect x="26" y="34" width="34" height="9" rx="4.5" fill="currentColor" opacity=".8"/><rect x="16" y="59.5" width="44" height="9" rx="4.5" fill="currentColor"/><rect x="26" y="85" width="34" height="9" rx="4.5" fill="currentColor" opacity=".8"/><path d="M72 22 L122 64 L72 106 Z" fill="currentColor" opacity=".9"/></svg>';
    h += '<h2>A guided tour through code you didn&rsquo;t write</h2>';
    h += '<p>Diffs, AI-generated changes, or any function you want to understand — stepped through right here in your editor.</p>';
    h += '</div>';
    h += '<div class="card stack">';
    h += '<button data-act="start" data-mode="instant">' + I.zap + 'Instant tour — local, no AI, &lt;1s</button>';
    h += '<button data-act="start" data-mode="diff">' + I.diff + 'Tour working tree changes</button>';
    h += '<button data-act="start" data-mode="staged">' + I.diff + 'Tour staged changes</button>';
    h += '<button data-act="start" data-mode="selection">' + I.cursor + 'Tour code path at cursor</button>';
    h += '</div>';
    h += '<div class="card"><div class="row spread"><span class="muted">Style</span>';
    h += styleSeg();
    h += '</div><div class="kv muted">Explanations tuned per audience — Learner explains syntax &amp; jargon for AI-generated code.</div></div>';
    return h;
  }

  function styleSeg() {
    var opts = [['expert', 'Expert'], ['standard', 'Std'], ['learner', 'Learner']];
    var h = '<div class="seg">';
    for (var i = 0; i < opts.length; i++) {
      h += '<button data-act="style" data-s="' + opts[i][0] + '" class="' + (state.style === opts[i][0] ? 'on' : '') + '">' + opts[i][1] + '</button>';
    }
    return h + '</div>';
  }

  function renderPlan() {
    var p = state.plan;
    var cur = p.steps[state.current] || p.steps[0];
    var h = '';

    h += '<div class="card">';
    h += '<div class="row spread"><div class="seg">' +
      [['expert','Expert'],['standard','Std'],['learner','Learner']].map(function(o){
        return '<button data-act="style" data-s="' + o[0] + '" class="' + (state.style === o[0] ? 'on' : '') + '">' + o[1] + '</button>';
      }).join('') + '</div>';
    h += '<button class="icon sec" data-act="exit" title="Exit walkthrough">' + I.exit + '</button></div>';
    h += '<div class="title" style="margin-top:10px">' + esc(cur.title) + '</div>';
    h += '<div class="summary">' + md(p.summary) + '</div>';
    h += '<div class="kv"><span class="badge mono">' + esc(p.entryPoint) + '</span></div>';
    h += '</div>';

    h += '<div class="card">';
    h += '<div class="row">';
    h += '<button class="icon" data-act="prev" ' + (state.current === 0 ? 'disabled' : '') + ' title="Previous (alt+[)">' + I.chevL + '</button>';
    h += '<button class="icon ' + (state.autoplay ? 'sec' : '') + '" data-act="autoplay" title="Toggle autoplay">' + (state.autoplay ? I.pause : I.play) + '</button>';
    h += '<button class="icon" data-act="next" ' + (state.current >= p.totalSteps - 1 ? 'disabled' : '') + ' title="Next (alt+])">' + I.chevR + '</button>';
    h += '<div class="grow"></div>';
    h += '<span class="counter">Step ' + (state.current + 1) + ' of ' + p.totalSteps + '</span>';
    h += '</div>';
    h += '<div class="progress">';
    for (var i = 0; i < p.totalSteps; i++) {
      var cls = i === state.current ? 'cur' : i < state.current ? 'done' : '';
      h += '<div class="bar ' + cls + '" data-act="jump" data-i="' + i + '" title="' + esc((i + 1) + '. ' + p.steps[i].title) + '"></div>';
    }
    h += '</div>';
    h += '</div>';

    h += '<div class="card">';
    h += '<div class="row"><span class="idx">' + (state.current + 1) + '</span>';
    if (cur.variable) h += '<span class="badge">' + actionGlyph(cur.variable.action) + esc(cur.variable.action) + '</span>';
    h += '<span class="badge mono">' + esc(cur.filePath.split('/').pop()) + ':' + cur.range.startLine + '</span>';
    h += '</div>';
    if (cur.variable) {
      var v = cur.variable;
      var hasT = v.stateBefore !== undefined || v.stateAfter !== undefined;
      if (hasT) {
        h += '<div class="transition">';
        h += '<div class="state"><span class="lbl">' + esc(v.name) + ' · BEFORE</span>' + esc(v.stateBefore == null ? '?' : v.stateBefore) + '</div>';
        h += '<span class="arrow">&#10132;</span>';
        h += '<div class="state"><span class="lbl">AFTER</span>' + esc(v.stateAfter == null ? '?' : v.stateAfter) + '</div>';
        h += '</div>';
      } else {
        h += '<div class="kv"><span class="badge mono">' + esc(v.name) + '</span> <span class="badge">' + esc(v.action) + '</span></div>';
      }
    }
    h += '<div class="expl">' + md(cur.explanation) + '</div>';
    if (cur.securityNote) {
      h += '<div class="warn">' + I.shield + '<div><strong>Security</strong> — ' + md(cur.securityNote) + '</div></div>';
    }
    h += '</div>';

    h += '<div class="card steps">';
    h += '<div class="row spread" style="margin-bottom:6px"><span class="muted">Steps</span>';
    h += '<button class="sec" style="padding:4px 10px" data-act="browse">' + I.list + 'Browse</button></div>';
    for (var j = 0; j < p.steps.length; j++) {
      var s = p.steps[j];
      h += '<div class="step-row' + (j === state.current ? ' cur' : '') + '" data-act="jump" data-i="' + j + '">';
      h += '<span class="idx">' + (j + 1) + '</span>';
      h += '<span class="t">' + esc(s.title) + '</span>';
      if (s.variable) h += '<span class="badge mono">' + esc(s.variable.name) + '</span>';
      if (s.securityNote) h += '<span class="dot" title="security note"></span>';
      h += '</div>';
    }
    h += '</div>';

    return h;
  }

  render();
})();
</script>
</body>
</html>`;
}
