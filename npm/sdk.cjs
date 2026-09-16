'use strict';

// Reference-only Node client. Rust retains pairing, custody and human consent.
const { execFile } = require('node:child_process');
const { randomUUID } = require('node:crypto');
const fs = require('node:fs/promises');
const os = require('node:os');
const path = require('node:path');
const { setTimeout: delay } = require('node:timers/promises');
const { performance } = require('node:perf_hooks');
const { resolveBinary } = require('./launcher.cjs');

const nativeErrors = new Set(['invalid_request', 'unsupported_version', 'unauthorized',
  'stale_session', 'busy', 'denied', 'unavailable', 'not_found', 'conflict', 'capacity',
  'expired', 'persistence_uncertain', 'transport_unavailable', 'transport_uncertain',
  'unsupported_target', 'ambiguous_target', 'stale_target', 'permission_denied', 'cancelled']);
const errorCodes = new Set([...nativeErrors, 'native_unavailable', 'wait_timeout']);
class MagicVaultError extends Error {
  constructor(code, operationId) {
    const safeCode = errorCodes.has(code) ? code : 'unavailable';
    super(`MagicVault: ${safeCode}`);
    this.name = 'MagicVaultError';
    this.code = safeCode;
    if (uuid(operationId)) this.operationId = operationId;
  }
}
const invalid = () => { throw new MagicVaultError('invalid_request'); };
const uuid = v => typeof v === 'string' && /^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/.test(v);
const operation = v => uuid(v) && v !== '00000000-0000-0000-0000-000000000000';
const reference = v => typeof v === 'string' && v.startsWith('cred_') && uuid(v.slice(5));
const text = (max, min = 1) => v => typeof v === 'string' && v.length >= min && Buffer.byteLength(v) <= max && !/[\x00-\x1f\x7f]/.test(v);
const name = v => typeof v === 'string' && /^[A-Za-z0-9_-]{1,64}$/.test(v);
const bool = v => typeof v === 'boolean';
const integer = v => Number.isSafeInteger(v) && v >= 0;
const oneOf = (...items) => v => items.includes(v);
const nullable = rule => v => v === null || rule(v);
const array = (rule, max, min = 0) => v => Array.isArray(v) && v.length >= min && v.length <= max && v.every(rule);
const shape = (required, optional = {}) => v => v !== null && typeof v === 'object' && !Array.isArray(v)
  && Object.keys(v).every(k => Object.hasOwn(required, k) || Object.hasOwn(optional, k))
  && Object.entries(required).every(([k, rule]) => Object.hasOwn(v, k) && rule(v[k]))
  && Object.entries(optional).every(([k, rule]) => !Object.hasOwn(v, k) || v[k] === undefined || rule(v[k]));
const receiptError = nullable(v => nativeErrors.has(v));
const fillSchema = shape({ operation_id: uuid,
  state: oneOf('pending', 'filling', 'filled', 'denied', 'cancelled', 'expired', 'failed', 'partial', 'uncertain'),
  fields: array(oneOf('filled', 'not_filled', 'uncertain'), 8), error: receiptError });
const deliverySchema = shape({ operation_id: uuid, kind: oneOf('process', 'http'),
  state: oneOf('pending', 'running', 'completed', 'denied', 'cancelled', 'expired', 'failed', 'uncertain'),
  may_have_run: bool, error: receiptError });
const schemas = {
  status: shape({ epoch: uuid, client_id: nullable(uuid), ready: bool, effects: array(text(64), 16) }),
  credentials: array(shape({ credential_ref: reference, label: text(80), field_names: array(name, 8, 1) }), 256),
  approval: shape({ approval_id: uuid, decision: oneOf('pending', 'allowed', 'denied', 'expired', 'uncertain'), operation: oneOf('metadata_connection') }),
  browsers: array(shape({ browser_handle: uuid, label: text(80), backend: oneOf('cdp', 'extension') }), 8),
  browser_targets: array(shape({ target_handle: uuid, browser_handle: uuid, tab_id: text(256), frame_id: text(256),
    origin: text(256), top_origin: text(256), is_main_frame: bool, expires_in_seconds: integer }), 128),
  fill: fillSchema,
  delivery_profiles: array(shape({ profile_id: uuid, label: text(80), kind: oneOf('process', 'http') }), 16),
  delivery: deliverySchema,
};
const callOptions = shape({}, { signal: v => v instanceof AbortSignal });

class MagicVault {
  #executable; #prefix; #timeout;
  constructor(options = {}) {
    if (!shape({}, { executable: v => text(4096)(v) && path.isAbsolute(v), root: v => text(4096)(v) && path.isAbsolute(v),
      profile: v => name(v) && !v.startsWith('-'), timeoutMs: v => Number.isInteger(v) && v >= 1 && v <= 120000 })(options)) invalid();
    this.#executable = options.executable;
    this.#prefix = ['--profile', options.profile ?? 'default'];
    if (options.root) this.#prefix.push('--root', options.root);
    this.#timeout = options.timeoutMs ?? 10000;
  }

  async #call(command, args, kind, options = {}, operationId, deadlineMs) {
    if (!callOptions(options)) invalid();
    if (options.signal?.aborted) throw new MagicVaultError('cancelled', operationId);
    let executable = this.#executable;
    if (!executable) {
      try { executable = resolveBinary('magicvault'); }
      catch { throw new MagicVaultError('native_unavailable', operationId); }
    }
    return new Promise((resolve, reject) => {
      execFile(executable, [...this.#prefix, command, ...args], {
        shell: false, encoding: 'utf8', maxBuffer: 256 * 1024,
        timeout: Math.max(1, Math.floor(Math.min(this.#timeout, deadlineMs ?? this.#timeout))),
        killSignal: 'SIGKILL', signal: options.signal,
      }, (error, stdout, stderr) => {
        const fail = code => reject(new MagicVaultError(code, operationId));
        if (error) {
          // Never attach the child-process Error, path, arguments or raw streams.
          if (error.code === 'ENOENT' || error.code === 'EACCES') return fail('native_unavailable');
          if (Number.isInteger(error.code) && !error.signal && !error.killed) {
            try {
              const value = JSON.parse(stderr);
              if (shape({ error: v => nativeErrors.has(v) })(value)) return fail(value.error);
            } catch { /* Closed error below; no raw diagnostics escape. */ }
          }
          // Killing this client does not cancel an operation in the daemon.
          return fail('transport_uncertain');
        }
        try {
          const value = JSON.parse(stdout);
          if (stderr || !shape({ kind: oneOf(kind), data: schemas[kind] })(value)
              || (operationId && value.data.operation_id !== operationId)) return fail('transport_uncertain');
          resolve(value.data);
        } catch { fail('transport_uncertain'); }
      });
    });
  }

  status(options) { return this.#call('status', [], 'status', options); }
  listCredentials(options) { return this.#call('list-credentials', [], 'credentials', options); }
  listBrowsers(options) { return this.#call('list-browsers', [], 'browsers', options); }
  listDeliveryProfiles(options) { return this.#call('list-delivery-profiles', [], 'delivery_profiles', options); }
  async requestApproval(credentialRef, options) {
    if (!reference(credentialRef)) invalid();
    return this.#call('request-access', ['--credential-ref', credentialRef], 'approval', options);
  }
  async approvalStatus(approvalId, options) {
    if (!uuid(approvalId)) invalid();
    const result = await this.#call('approval-status', ['--approval-id', approvalId], 'approval', options);
    if (result.approval_id !== approvalId) throw new MagicVaultError('transport_uncertain');
    return result;
  }
  async browserTargets(query, options) {
    if (!shape({ browser_handle: uuid }, { top_origin: text(256), tab_id: text(256) })(query)) invalid();
    const { browser_handle, top_origin, tab_id } = query;
    const args = ['--browser-handle', browser_handle];
    if (top_origin) args.push('--top-origin', top_origin);
    if (tab_id) args.push('--tab-id', tab_id);
    const result = await this.#call('browser-targets', args, 'browser_targets', options);
    if (result.some(t => t.browser_handle !== browser_handle
      || (top_origin && t.top_origin !== top_origin) || (tab_id && t.tab_id !== tab_id))) {
      throw new MagicVaultError('transport_uncertain');
    }
    return result;
  }
  secureFill(request, options = {}) { return this.#fill('secure-fill', request, options); }
  securePromptFill(request, options = {}) { return this.#fill('secure-prompt-fill', request, options); }
  async #fill(command, request, options) {
    const prompted = command === 'secure-prompt-fill';
    const css = v => text(512)(v) && !!v.trim() && /^[ -~]+$/.test(v);
    const field = prompted ? shape({ css, field_name: name })
      : shape({ css, credential_ref: reference, credential_field: name });
    if (!shape({ operation_id: operation, browser_handle: prompted ? operation : uuid,
      target_handle: prompted ? operation : uuid, fields: array(field, 8, 1) })(request)
        || new Set(request.fields.map(f => f.css)).size !== request.fields.length
        || (prompted && new Set(request.fields.map(f => f.field_name)).size !== request.fields.length)
        || !callOptions(options)) invalid();
    // Snapshot metadata before the first await; values never enter this API.
    const operationId = request.operation_id;
    const body = JSON.stringify({ operation_id: operationId, browser_handle: request.browser_handle,
      target_handle: request.target_handle, fields: request.fields.map(f => prompted
        ? { css: f.css, field_name: f.field_name }
        : { css: f.css, credential_ref: f.credential_ref, credential_field: f.credential_field }) });
    let directory;
    try {
      if (options.signal?.aborted) throw new MagicVaultError('cancelled', operationId);
      directory = await fs.mkdtemp(path.join(os.tmpdir(), 'magicvault-sdk-'));
      await fs.chmod(directory, 0o700);
      const file = path.join(directory, 'request.json');
      await fs.writeFile(file, body, { mode: 0o600, flag: 'wx' });
      return await this.#call(command, ['--request-file', file], 'fill', options, operationId);
    } catch (error) {
      if (error instanceof MagicVaultError) throw error;
      throw new MagicVaultError('unavailable', operationId);
    } finally {
      // Only our private reference-only file. Cleanup failure must not obscure a receipt.
      if (directory) await fs.rm(directory, { recursive: true, force: true }).catch(() => {});
    }
  }
  async #delivery(command, request, options) {
    if (!shape({ operation_id: operation, profile_id: operation })(request)) invalid();
    const { operation_id, profile_id } = request;
    const result = await this.#call(command, ['--profile-id', profile_id, '--operation-id', operation_id],
      'delivery', options, operation_id);
    if (result.kind !== (command === 'secure-new-http' ? 'http' : 'process')) {
      throw new MagicVaultError('transport_uncertain', operation_id);
    }
    return result;
  }
  secureNewHttp(request, options) { return this.#delivery('secure-new-http', request, options); }
  secureNewProcess(request, options) { return this.#delivery('secure-new-process', request, options); }
  async #operationCall(command, kind, operationId, options, deadlineMs) {
    if (!operation(operationId)) invalid();
    return this.#call(command, ['--operation-id', operationId], kind, options, operationId, deadlineMs);
  }
  fillStatus(id, options) { return this.#operationCall('fill-status', 'fill', id, options); }
  cancelFill(id, options) { return this.#operationCall('cancel-fill', 'fill', id, options); }
  deliveryStatus(id, options) { return this.#operationCall('delivery-status', 'delivery', id, options); }
  cancelDelivery(id, options) { return this.#operationCall('cancel-delivery', 'delivery', id, options); }
  async #wait(kind, id, options = {}) {
    if (!operation(id) || !shape({}, { signal: v => v instanceof AbortSignal,
      timeoutMs: v => Number.isInteger(v) && v >= 1 && v <= 600000,
      intervalMs: v => Number.isInteger(v) && v >= 50 && v <= 10000 })(options)) invalid();
    const deadline = performance.now() + (options.timeoutMs ?? 60000);
    for (;;) {
      if (options.signal?.aborted) throw new MagicVaultError('cancelled', id);
      const remaining = deadline - performance.now();
      if (remaining <= 0) throw new MagicVaultError('wait_timeout', id);
      let receipt;
      try {
        receipt = await this.#operationCall(`${kind}-status`, kind, id, { signal: options.signal }, remaining);
      } catch (error) {
        if (performance.now() >= deadline && error.code === 'transport_uncertain') throw new MagicVaultError('wait_timeout', id);
        throw error;
      }
      if (!['pending', 'filling', 'running'].includes(receipt.state)) return receipt;
      const wait = Math.min(options.intervalMs ?? 250, deadline - performance.now());
      if (wait <= 0) throw new MagicVaultError('wait_timeout', id);
      try { await delay(wait, undefined, { signal: options.signal }); }
      catch { throw new MagicVaultError('cancelled', id); }
    }
  }
  waitForFill(id, options) { return this.#wait('fill', id, options); }
  waitForDelivery(id, options) { return this.#wait('delivery', id, options); }
}

exports.MagicVault = MagicVault;
exports.MagicVaultError = MagicVaultError;
exports.createOperationId = randomUUID;
