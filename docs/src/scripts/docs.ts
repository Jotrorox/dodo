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
    status.textContent = 'Type to search all documentation.';
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

document.querySelector('.theme-toggle')?.addEventListener('click', () => {
  const theme = document.documentElement.dataset.theme === 'dark' ? 'light' : 'dark';
  document.documentElement.dataset.theme = theme;
  try { localStorage.setItem('dodo-theme', theme); } catch { /* Theme still works without storage. */ }
});

const navigation = document.querySelector<HTMLDetailsElement>('.navigation')!;
const mobile = matchMedia('(max-width: 760px)');
const updateNavigation = () => { navigation.open = !mobile.matches; };
updateNavigation();
mobile.addEventListener('change', updateNavigation);

const tocLinks = [...document.querySelectorAll<HTMLAnchorElement>('.toc a[href^="#"]')];
const headingLinks = new Map(tocLinks.map((link) => [decodeURIComponent(link.hash.slice(1)), link]));
const observer = new IntersectionObserver((entries) => {
  const visible = entries.filter((entry) => entry.isIntersecting);
  if (!visible.length) return;
  const active = visible.sort((a, b) => a.boundingClientRect.top - b.boundingClientRect.top)[0].target.id;
  tocLinks.forEach((link) => link.removeAttribute('aria-current'));
  headingLinks.get(active)?.setAttribute('aria-current', 'location');
}, { rootMargin: '-80px 0px -65% 0px' });
document.querySelectorAll('.prose h2[id], .prose h3[id]').forEach((heading) => observer.observe(heading));
