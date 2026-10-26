type SearchEntry = { title: string; heading: string; url: string; text: string };

const searchDialog = document.querySelector<HTMLDialogElement>('#search-dialog')!;
const shortcutsDialog = document.querySelector<HTMLDialogElement>('#shortcuts-dialog')!;
const input = document.querySelector<HTMLInputElement>('#search-input')!;
const results = document.querySelector<HTMLUListElement>('#search-results')!;
const status = document.querySelector<HTMLElement>('#search-status')!;
let index: SearchEntry[] | undefined;
let loading: Promise<void> | undefined;
let matches: SearchEntry[] = [];
let selected = -1;
let returnFocus: HTMLElement | null = null;

if (/Mac|iPhone|iPad/.test(navigator.platform)) {
  document.querySelectorAll('[data-search-key]').forEach((key) => { key.textContent = '⌘ K'; });
}

function selectResult(position: number, scroll = true) {
  selected = position;
  [...results.children].forEach((result, index) => result.setAttribute('aria-selected', String(index === selected)));
  const active = results.children[selected];
  if (active) {
    input.setAttribute('aria-activedescendant', active.id);
    if (scroll) active.scrollIntoView({ block: 'nearest' });
  } else {
    input.removeAttribute('aria-activedescendant');
  }
}

function search() {
  const query = input.value.trim().toLocaleLowerCase();
  results.replaceChildren();
  input.setAttribute('aria-expanded', 'false');
  matches = [];
  selectResult(-1);
  if (!index) return;
  if (!query) {
    status.textContent = 'Search every guide, reference page, and API declaration.';
    return;
  }
  const terms = query.split(/\s+/);
  const ranked = index.map((entry) => {
    const title = entry.title.toLocaleLowerCase();
    const heading = entry.heading.toLocaleLowerCase();
    const text = entry.text.toLocaleLowerCase();
    const all = `${title} ${heading} ${text}`;
    if (!terms.every((term) => all.includes(term))) return { entry, score: 0 };
    const score = terms.reduce((total, term) => total + (heading.includes(term) ? 12 : 0) + (title.includes(term) ? 6 : 0) + (text.includes(term) ? 1 : 0), 0)
      + (heading.includes(query) ? 20 : 0) + (title === query ? 30 : 0);
    return { entry, score };
  }).filter(({ score }) => score > 0).sort((a, b) => b.score - a.score);
  matches = ranked.slice(0, 20).map(({ entry }) => entry);
  status.textContent = ranked.length
    ? `${ranked.length} result${ranked.length === 1 ? '' : 's'}${ranked.length > 20 ? ', showing the first 20' : ''}.`
    : 'No results. Try a different word or a shorter search.';
  matches.forEach((entry, position) => {
    const item = document.createElement('li');
    item.id = `search-result-${position}`;
    item.setAttribute('role', 'option');
    item.setAttribute('aria-selected', 'false');
    const link = document.createElement('a');
    link.href = entry.url;
    link.tabIndex = -1;
    const title = document.createElement('strong');
    title.textContent = entry.heading || entry.title;
    const context = document.createElement('span');
    context.className = 'result-context';
    context.textContent = entry.heading ? entry.title : 'Overview';
    const excerpt = document.createElement('p');
    const found = entry.text.toLocaleLowerCase().indexOf(terms.find((term) => entry.text.toLocaleLowerCase().includes(term)) ?? '');
    const start = Math.max(0, found - 50);
    excerpt.textContent = `${start ? '…' : ''}${entry.text.slice(start, start + 180)}${entry.text.length > start + 180 ? '…' : ''}`;
    link.append(context, title, excerpt);
    item.append(link);
    item.addEventListener('pointermove', () => selectResult(position, false));
    link.addEventListener('click', () => searchDialog.close());
    results.append(item);
  });
  input.setAttribute('aria-expanded', String(matches.length > 0));
  selectResult(matches.length ? 0 : -1);
}

async function loadIndex() {
  if (index) return;
  if (loading) return loading;
  status.textContent = 'Loading search…';
  loading = (async () => {
    try {
      const response = await fetch(searchDialog.dataset.searchIndex!);
      if (!response.ok) throw new Error('Search index unavailable');
      index = await response.json();
      search();
    } catch {
      status.textContent = 'Search could not load. Close and reopen search to try again.';
    } finally {
      loading = undefined;
    }
  })();
  return loading;
}

function openDialog(dialog: HTMLDialogElement) {
  if (dialog.open) return;
  if (!searchDialog.open && !shortcutsDialog.open) {
    returnFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  }
  for (const other of [searchDialog, shortcutsDialog]) if (other.open) other.close();
  dialog.showModal();
  if (dialog === searchDialog) {
    input.focus();
    input.select();
    void loadIndex();
  }
}

document.querySelectorAll('[data-search-open]').forEach((button) => button.addEventListener('click', () => openDialog(searchDialog)));
document.querySelectorAll('[data-shortcuts-open]').forEach((button) => button.addEventListener('click', () => openDialog(shortcutsDialog)));
document.querySelectorAll<HTMLButtonElement>('[data-close-dialog]').forEach((button) => button.addEventListener('click', () => button.closest('dialog')?.close()));
for (const dialog of [searchDialog, shortcutsDialog]) {
  dialog.addEventListener('click', (event) => {
    if (event.target !== dialog) return;
    const rect = dialog.getBoundingClientRect();
    if (event.clientX < rect.left || event.clientX > rect.right || event.clientY < rect.top || event.clientY > rect.bottom) dialog.close();
  });
  dialog.addEventListener('close', () => {
    if (!searchDialog.open && !shortcutsDialog.open) returnFocus?.focus();
  });
}

input.addEventListener('input', search);
input.addEventListener('keydown', (event) => {
  if (event.isComposing) return;
  if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
    event.preventDefault();
    if (matches.length) selectResult((selected + (event.key === 'ArrowDown' ? 1 : -1) + matches.length) % matches.length);
  } else if (event.key === 'Enter' && matches[selected]) {
    event.preventDefault();
    const url = matches[selected].url;
    searchDialog.close();
    window.location.assign(url);
  }
});

document.addEventListener('keydown', (event) => {
  if (event.isComposing || event.defaultPrevented) return;
  if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k' && !event.altKey) {
    event.preventDefault();
    openDialog(searchDialog);
    return;
  }
  const target = event.target;
  const typing = target instanceof HTMLElement && (target.isContentEditable || !!target.closest('input, textarea, select, [role="textbox"]'));
  if (typing || event.metaKey || event.ctrlKey || event.altKey || searchDialog.open || shortcutsDialog.open) return;
  if (event.key === '/' || event.key === '?') {
    event.preventDefault();
    openDialog(event.key === '/' ? searchDialog : shortcutsDialog);
  }
});

// Appearance follows the system unless someone picks light or dark explicitly.
type ThemeMode = 'system' | 'light' | 'dark';
const themeLabels: Record<ThemeMode, string> = { system: 'match system', light: 'light', dark: 'dark' };
const themeToggle = document.querySelector<HTMLButtonElement>('[data-theme-toggle]')!;
const themeColors = document.querySelectorAll<HTMLMetaElement>('meta[name="theme-color"]');
function applyTheme(mode: ThemeMode) {
  const root = document.documentElement;
  root.dataset.themeMode = mode;
  if (mode === 'system') delete root.dataset.theme; else root.dataset.theme = mode;
  const color = mode === 'system' ? null : mode === 'dark' ? '#161618' : '#ffffff';
  themeColors.forEach((meta) => meta.setAttribute('content', color ?? (meta.media.includes('dark') ? '#161618' : '#ffffff')));
  const label = `Appearance: ${themeLabels[mode]}`;
  themeToggle.setAttribute('aria-label', label);
  themeToggle.title = label;
}
applyTheme((document.documentElement.dataset.theme as ThemeMode | undefined) ?? 'system');
themeToggle.addEventListener('click', () => {
  const order: ThemeMode[] = ['system', 'light', 'dark'];
  const mode = order[(order.indexOf(document.documentElement.dataset.themeMode as ThemeMode) + 1) % order.length];
  applyTheme(mode);
  try {
    if (mode === 'system') localStorage.removeItem('dodo-theme'); else localStorage.setItem('dodo-theme', mode);
  } catch { /* The choice still applies to this page without storage. */ }
});

// At compact widths the sidebar is a sheet over the page.
const sidebar = document.querySelector<HTMLElement>('#sidebar')!;
const scrim = document.querySelector<HTMLElement>('.sidebar-scrim')!;
const menuToggle = document.querySelector<HTMLButtonElement>('[data-menu-toggle]')!;
const compact = matchMedia('(max-width: 820px)');
function setNavigation(open: boolean, restoreFocus = true) {
  document.body.classList.toggle('nav-open', open);
  scrim.hidden = !open;
  menuToggle.setAttribute('aria-expanded', String(open));
  menuToggle.setAttribute('aria-label', open ? 'Hide navigation' : 'Show navigation');
  document.querySelectorAll<HTMLElement>('main, .toc').forEach((region) => { region.inert = open; });
  if (open) {
    (sidebar.querySelector<HTMLElement>('[aria-current="page"]') ?? sidebar.querySelector<HTMLElement>('a, summary'))?.focus();
  } else if (restoreFocus && sidebar.contains(document.activeElement)) {
    menuToggle.focus();
  }
}
menuToggle.addEventListener('click', () => setNavigation(!document.body.classList.contains('nav-open')));
scrim.addEventListener('click', () => setNavigation(false));
compact.addEventListener('change', () => setNavigation(false, false));
document.addEventListener('keydown', (event) => {
  if (event.key === 'Escape' && document.body.classList.contains('nav-open')) setNavigation(false);
});

// Keep the current page visible even when the API directory is long. Scroll
// only the sidebar, preserving the document's heading anchor and reading position.
const currentPage = sidebar.querySelector<HTMLElement>('[aria-current="page"]');
if (!compact.matches && currentPage) {
  const item = currentPage.getBoundingClientRect();
  const panel = sidebar.getBoundingClientRect();
  if (item.top < panel.top || item.bottom > panel.bottom) {
    sidebar.scrollTop += item.top - panel.top - (sidebar.clientHeight - item.height) / 2;
  }
}

// Code blocks get a language label and a copy control.
const languageNames: Record<string, string> = {
  dodo: 'Dodo', sh: 'Shell', powershell: 'PowerShell', toml: 'TOML', json: 'JSON',
  yaml: 'YAML', python: 'Python', rust: 'Dodo', text: 'Text', ebnf: 'EBNF',
};
document.querySelectorAll<HTMLPreElement>('.prose pre').forEach((pre) => {
  const block = document.createElement('div');
  block.className = 'code-block';
  const toolbar = document.createElement('div');
  toolbar.className = 'code-toolbar';
  const language = document.createElement('span');
  language.className = 'code-language';
  const name = pre.dataset.language ?? 'text';
  language.textContent = languageNames[name] ?? name;
  const copy = document.createElement('button');
  copy.type = 'button';
  copy.className = 'copy-button';
  copy.textContent = 'Copy';
  copy.setAttribute('aria-label', `Copy ${language.textContent} code`);
  let reset: number | undefined;
  copy.addEventListener('click', async () => {
    try {
      await navigator.clipboard.writeText(pre.innerText.replace(/\n$/, ''));
      copy.textContent = 'Copied';
      copy.dataset.copied = '';
    } catch {
      copy.textContent = 'Copy failed';
    }
    window.clearTimeout(reset);
    reset = window.setTimeout(() => { copy.textContent = 'Copy'; delete copy.dataset.copied; }, 2000);
  });
  toolbar.append(language, copy);
  pre.replaceWith(block);
  block.append(toolbar, pre);
});

// Headings link to themselves.
document.querySelectorAll<HTMLHeadingElement>('.prose :is(h2, h3, h4)[id]').forEach((heading) => {
  const anchor = document.createElement('a');
  anchor.className = 'heading-anchor';
  anchor.href = `#${heading.id}`;
  anchor.textContent = '#';
  anchor.setAttribute('aria-label', `Link to “${heading.textContent}”`);
  heading.prepend(anchor);
});

const tocLinks = [...document.querySelectorAll<HTMLAnchorElement>('.toc a[href^="#"]:not(.back-to-top)')];
const headingLinks = new Map(tocLinks.map((link) => [decodeURIComponent(link.hash.slice(1)), link]));
const observer = new IntersectionObserver((entries) => {
  const visible = entries.filter((entry) => entry.isIntersecting);
  if (!visible.length) return;
  const active = visible.sort((a, b) => a.boundingClientRect.top - b.boundingClientRect.top)[0].target.id;
  tocLinks.forEach((link) => link.removeAttribute('aria-current'));
  headingLinks.get(active)?.setAttribute('aria-current', 'location');
}, { rootMargin: '-80px 0px -65% 0px' });
document.querySelectorAll('.prose h2[id], .prose h3[id]').forEach((heading) => observer.observe(heading));
