// Inline SVG icons, translated from the design handoff
// (`Deduplication app UI design.zip`). Every icon here is a fixed,
// hardcoded string -- never built from user-controlled data (file paths,
// scan results) -- so building it via a wrapper `<svg>`'s innerHTML is
// safe. Anything that touches real scan data uses DOM text APIs instead
// (see app.js) rather than string interpolation into HTML.
const ICONS = {
  dashboard:
    '<rect x="3" y="3" width="8" height="8" rx="1.5" stroke="currentColor" stroke-width="1.7"/><rect x="13" y="3" width="8" height="8" rx="1.5" stroke="currentColor" stroke-width="1.7"/><rect x="3" y="13" width="8" height="8" rx="1.5" stroke="currentColor" stroke-width="1.7"/><rect x="13" y="13" width="8" height="8" rx="1.5" stroke="currentColor" stroke-width="1.7"/>',
  scan:
    '<path d="M3 8V4h4M17 4h4v4M21 16v4h-4M7 20H3v-4" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/><circle cx="12" cy="12" r="3.4" stroke="currentColor" stroke-width="1.7"/>',
  review:
    '<path d="M12 3l8 4.5-8 4.5-8-4.5L12 3z" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round"/><path d="M4 12.5L12 17l8-4.5M4 16.5L12 21l8-4.5" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round"/>',
  rules:
    '<path d="M4 6h16M4 12h16M4 18h16" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"/><circle cx="9" cy="6" r="2" fill="var(--sidebar)" stroke="currentColor" stroke-width="1.7"/><circle cx="16" cy="12" r="2" fill="var(--sidebar)" stroke="currentColor" stroke-width="1.7"/><circle cx="7" cy="18" r="2" fill="var(--sidebar)" stroke="currentColor" stroke-width="1.7"/>',
  logo:
    '<circle cx="9" cy="12" r="6" stroke="#fff" stroke-width="1.8"/><circle cx="15" cy="12" r="6" stroke="#fff" stroke-width="1.8" opacity="0.55"/>',
  sun:
    '<circle cx="12" cy="12" r="3" stroke="currentColor" stroke-width="1.6"/><path d="M12 3v2M12 19v2M3 12h2M19 12h2M5.6 5.6l1.4 1.4M17 17l1.4 1.4M18.4 5.6L17 7M7 17l-1.4 1.4" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>',
  moon:
    '<path d="M20 14.5A8 8 0 1 1 9.5 4a6.5 6.5 0 0 0 10.5 10.5z" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/>',
  chevronLeft:
    '<path d="M15 18l-6-6 6-6" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>',
  chevronRight:
    '<path d="M9 18l6-6-6-6" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>',
  folder:
    '<path d="M3 6a1 1 0 011-1h5l2 2h9a1 1 0 011 1v10a1 1 0 01-1 1H4a1 1 0 01-1-1V6z" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/>',
  file:
    '<path d="M6 2h9l5 5v15H6V2z" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/><path d="M15 2v5h5" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/>',
  check:
    '<path d="M4 12l6 6L20 6" stroke="#fff" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"/>',
  shield:
    '<path d="M12 3l7 3v6c0 4.5-3 7-7 9-4-2-7-4.5-7-9V6l7-3z" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/>',
  scanEmpty:
    '<path d="M3 8V4h4M17 4h4v4M21 16v4h-4M7 20H3v-4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/><circle cx="12" cy="12" r="4" stroke="currentColor" stroke-width="1.5"/>',
  similar:
    '<path d="M8 8h9v9H8z" stroke="currentColor" stroke-width="1.6"/><path d="M5 5h9v9" stroke="currentColor" stroke-width="1.6" opacity="0.5"/>',
};

function icon(name, size) {
  size = size || 16;
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("width", size);
  svg.setAttribute("height", size);
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("fill", "none");
  svg.innerHTML = ICONS[name] || "";
  return svg;
}
