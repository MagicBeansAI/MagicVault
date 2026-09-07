function magicvaultFill(expectedOrigin, fields) {
  if (!Array.isArray(fields) || fields.length === 0 || fields.length > 8)
    return {fields: [], error: "invalid_request"};
  const result = fields.map(() => "not_filled");
  const fail = (error) => ({fields: result, error});
  const usable = (node, value) => node instanceof HTMLInputElement &&
    ["text", "password", "email", "tel", "url", "search"].includes(node.type) &&
    !node.disabled && !node.matches(":disabled") && !node.readOnly && node.isConnected &&
    node.getClientRects().length > 0 && getComputedStyle(node).visibility === "visible" &&
    !node.closest("[inert]") && (node.maxLength < 0 || value.length <= node.maxLength);
  try {
    // URL origin alone does not establish a document's security origin (for
    // example, a sandboxed opaque document can retain an HTTPS location).
    if (globalThis.origin !== expectedOrigin || location.origin !== expectedOrigin ||
        document.prerendering || !document.defaultView)
      return fail("stale_target");
    if (!Array.isArray(fields) || fields.length === 0 || fields.length > 8)
      return fail("invalid_request");
    const nodes = [];
    // Resolve and validate every field before writing any material.
    for (const field of fields) {
      if (typeof field.css !== "string" || field.css.length > 512 ||
          typeof field.value !== "string" || field.value.length === 0 || field.value.length > 4096)
        return fail("invalid_request");
      let matches;
      try { matches = document.querySelectorAll(field.css); }
      catch (_) { return fail("unsupported_target"); }
      if (matches.length !== 1) return fail(matches.length ? "ambiguous_target" : "stale_target");
      const node = matches[0];
      if (!usable(node, field.value))
        return fail("unsupported_target");
      if (nodes.includes(node)) return fail("ambiguous_target");
      nodes.push(node);
    }
    const setValue = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set;
    for (let index = 0; index < nodes.length; index++) {
      const node = nodes[index];
      // Earlier input/change handlers may replace or detach a later field.
      const matches = document.querySelectorAll(fields[index].css);
      if (globalThis.origin !== expectedOrigin || location.origin !== expectedOrigin || !node.isConnected ||
          matches.length !== 1 || matches[0] !== node || node.disabled || node.readOnly)
        return fail("stale_target");
      if (!usable(node, fields[index].value)) return fail("unsupported_target");
      // A setter/event exception can follow a write. Never imply rollback.
      result[index] = "uncertain";
      setValue.call(node, fields[index].value);
      node.dispatchEvent(new Event("input", {bubbles: true, composed: true}));
      node.dispatchEvent(new Event("change", {bubbles: true}));
      if (!node.isConnected || node.value !== fields[index].value) return fail("stale_target");
      result[index] = "filled";
    }
    return {fields: result, error: null};
  } catch (_) {
    return fail("unavailable");
  } finally {
    // Best effort lifetime reduction, not a JS heap erasure guarantee.
    for (const field of fields) if (field && typeof field === "object") field.value = "";
  }
}
