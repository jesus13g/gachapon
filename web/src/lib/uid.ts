/** Build-time unique ids for SVG defs (masks, gradients) that a component renders many times on one page. */
let n = 0;
export const uid = (prefix: string) => `${prefix}-${++n}`;
