import type { IconName } from '../../types';

interface IconProps {
  name:   IconName;
  size?:  number;
  color?: string;
}

export function Icon({ name, size = 14, color = 'currentColor' }: IconProps) {
  const s = { width: size, height: size, display: 'block' as const };
  const p = {
    strokeWidth:    '1.5',
    strokeLinecap:  'round' as const,
    strokeLinejoin: 'round' as const,
    fill:           'none',
    stroke:         color,
  };

  const icons: Record<IconName, React.ReactElement> = {
    lock:     <svg {...s} viewBox="0 0 16 16" {...p}><rect x="3" y="7" width="10" height="8" rx="1.5"/><path d="M5 7V5a3 3 0 016 0v2"/></svg>,
    unlock:   <svg {...s} viewBox="0 0 16 16" {...p}><rect x="3" y="7" width="10" height="8" rx="1.5"/><path d="M5 7V5a3 3 0 016 0"/></svg>,
    eye:      <svg {...s} viewBox="0 0 16 16" {...p}><path d="M1 8s2.5-5 7-5 7 5 7 5-2.5 5-7 5-7-5-7-5z"/><circle cx="8" cy="8" r="2"/></svg>,
    eyeOff:   <svg {...s} viewBox="0 0 16 16" {...p}><path d="M1 1l14 14M6.7 6.7A2 2 0 0010 10M4 4C2.3 5.2 1 8 1 8s2.5 5 7 5c1.4 0 2.6-.4 3.7-.9M8 3c4.5 0 7 5 7 5s-.7 1.4-2 2.7"/></svg>,
    copy:     <svg {...s} viewBox="0 0 16 16" {...p}><rect x="5" y="5" width="8" height="9" rx="1.5"/><path d="M3 11V3a1 1 0 011-1h8"/></svg>,
    check:    <svg {...s} viewBox="0 0 16 16" {...p} strokeWidth="2"><path d="M2.5 8.5l4 4 7-8"/></svg>,
    plus:     <svg {...s} viewBox="0 0 16 16" {...p}><path d="M8 2v12M2 8h12"/></svg>,
    search:   <svg {...s} viewBox="0 0 16 16" {...p}><circle cx="6.5" cy="6.5" r="4.5"/><path d="M10 10l3.5 3.5"/></svg>,
    settings: <svg {...s} viewBox="0 0 16 16" {...p}><circle cx="8" cy="8" r="2.5"/><path d="M8 1v2M8 13v2M1 8h2m10 0h2M3.05 3.05l1.42 1.42M11.53 11.53l1.42 1.42M3.05 12.95l1.42-1.42M11.53 4.47l1.42-1.42"/></svg>,
    trash:    <svg {...s} viewBox="0 0 16 16" {...p}><path d="M2 4h12M5 4V2h6v2M6 7v5M10 7v5M3 4l1 9a1 1 0 001 1h6a1 1 0 001-1l1-9"/></svg>,
    edit:     <svg {...s} viewBox="0 0 16 16" {...p}><path d="M11 2l3 3-9 9H2v-3l9-9z"/></svg>,
    close:    <svg {...s} viewBox="0 0 16 16" {...p}><path d="M3 3l10 10M13 3L3 13"/></svg>,
    back:     <svg {...s} viewBox="0 0 16 16" {...p}><path d="M13 8H3M7 4L3 8l4 4"/></svg>,
    shield:   <svg {...s} viewBox="0 0 16 16" {...p}><path d="M8 1l6 2.5V8c0 3.5-2.5 5.5-6 7C2 13.5 2 11.5 2 8V3.5L8 1z"/></svg>,
    key:      <svg {...s} viewBox="0 0 16 16" {...p}><circle cx="5" cy="9" r="3.5"/><path d="M7.5 6.5l5.5-5.5m0 0l1.5 1.5M13 1l1.5 1.5M10 4.5l1.5 1.5"/></svg>,
    kbd:      <svg {...s} viewBox="0 0 16 16" {...p}><rect x="1" y="3" width="14" height="10" rx="2"/><path d="M4 7h1m2 0h1m2 0h1M5.5 10h5"/></svg>,
    timer:    <svg {...s} viewBox="0 0 16 16" {...p}><circle cx="8" cy="9" r="5.5"/><path d="M8 6v3l2 1M6 1h4M8 1v2"/></svg>,
    person:   <svg {...s} viewBox="0 0 16 16" {...p}><circle cx="8" cy="5" r="3"/><path d="M2 14c0-3.3 2.7-6 6-6s6 2.7 6 6"/></svg>,
    globe:    <svg {...s} viewBox="0 0 16 16" {...p}><circle cx="8" cy="8" r="6.5"/><path d="M8 1.5C6 4 5 6 5 8s1 4 3 6.5M8 1.5C10 4 11 6 11 8s-1 4-3 6.5M1.5 8h13"/></svg>,
    terminal: <svg {...s} viewBox="0 0 16 16" {...p}><rect x="1" y="2" width="14" height="12" rx="2"/><path d="M4 6l3 3-3 3M9 12h4"/></svg>,
    more:     <svg {...s} viewBox="0 0 16 16" fill={color} stroke="none"><circle cx="8" cy="3.5" r="1.3"/><circle cx="8" cy="8" r="1.3"/><circle cx="8" cy="12.5" r="1.3"/></svg>,
    tag:      <svg {...s} viewBox="0 0 16 16" {...p}><path d="M1 1h6l7 7-6 7-7-7V1z"/><circle cx="4.5" cy="4.5" r="1" fill={color} stroke="none"/></svg>,
    drag:     <svg {...s} viewBox="0 0 16 16" fill={color} stroke="none"><circle cx="5.5" cy="4" r="1.2"/><circle cx="10.5" cy="4" r="1.2"/><circle cx="5.5" cy="8" r="1.2"/><circle cx="10.5" cy="8" r="1.2"/><circle cx="5.5" cy="12" r="1.2"/><circle cx="10.5" cy="12" r="1.2"/></svg>,
    external: <svg {...s} viewBox="0 0 16 16" {...p}><path d="M7 2H2v12h12V9"/><path d="M10 2h4v4M14 2L8 8"/></svg>,
    export:   <svg {...s} viewBox="0 0 16 16" {...p}><path d="M8 1v9M4 6l4 4 4-4"/><path d="M2 12v2h12v-2"/></svg>,
    rename:   <svg {...s} viewBox="0 0 16 16" {...p}><path d="M3 8h10M10 5l3 3-3 3"/></svg>,
    note:        <svg {...s} viewBox="0 0 16 16" {...p}><rect x="3" y="1.5" width="10" height="13" rx="1.5"/><path d="M6 5.5h4M5 8h6M5 10.5h4"/></svg>,
    fingerprint: <svg {...s} viewBox="0 0 16 16" {...p}><path d="M8 2C5 2 2.5 4.2 2.5 7c0 2 .7 3.8 2 5.2M8 2c3 0 5.5 2.2 5.5 5 0 2-.7 3.8-2 5.2M8 4.5c1.7 0 3 1.1 3 2.5 0 1.8-.5 3.4-1.4 4.8M8 4.5C6.3 4.5 5 5.6 5 7c0 1.8.5 3.4 1.4 4.8M8 7v.1"/></svg>,
    refresh:     <svg {...s} viewBox="0 0 16 16" {...p}><path d="M8 2.5A5.5 5.5 0 1 0 13.5 8"/><path d="M13.5 5v3h-3"/></svg>,
  };

  return <span className="inline-flex items-center">{icons[name] ?? null}</span>;
}
