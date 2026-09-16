/** Reference-only Node API. Human setup, enrollment and consent remain native. */
export type UUID = string;
export type CredentialRef = `cred_${string}`;
export type ErrorCode = 'invalid_request' | 'unsupported_version' | 'unauthorized'
  | 'stale_session' | 'busy' | 'denied' | 'unavailable' | 'not_found' | 'conflict'
  | 'capacity' | 'expired' | 'persistence_uncertain' | 'transport_unavailable'
  | 'transport_uncertain' | 'unsupported_target' | 'ambiguous_target' | 'stale_target'
  | 'permission_denied' | 'cancelled';
export class MagicVaultError extends Error {
  constructor(code: ErrorCode | 'native_unavailable' | 'wait_timeout', operationId?: UUID);
  readonly code: ErrorCode | 'native_unavailable' | 'wait_timeout';
  /** Retain this ID. A lost response never authorizes replaying an effect. */
  readonly operationId?: UUID;
}
export interface MagicVaultOptions {
  /** Defaults to the matching, integrity-checked executable in the npm package. */
  executable?: string;
  /** Absolute vault root; defaults to the native CLI's user vault. */
  root?: string;
  /** Must already be paired by the human. Defaults to "default". */
  profile?: string;
  /** Per CLI call, 1–120000 ms. Default 10000; killing a client does not undo a delivery. */
  timeoutMs?: number;
}
export interface CallOptions { signal?: AbortSignal }
export interface WaitOptions extends CallOptions {
  /** Overall polling budget, 1–600000 ms. Default 60000. */
  timeoutMs?: number;
  /** Poll spacing, 50–10000 ms. Default 250. Only status reads are repeated. */
  intervalMs?: number;
}
export interface ServiceStatus { epoch: UUID; client_id: UUID | null; ready: boolean; effects: string[] }
export interface CredentialMetadata { credential_ref: CredentialRef; label: string; field_names: string[] }
export interface ApprovalStatus {
  approval_id: UUID;
  decision: 'pending' | 'allowed' | 'denied' | 'expired' | 'uncertain';
  operation: 'metadata_connection';
}
export interface BrowserInfo { browser_handle: UUID; label: string; backend: 'cdp' | 'extension' }
export interface BrowserTargetsQuery { browser_handle: UUID; top_origin?: string; tab_id?: string }
export interface BrowserTarget {
  target_handle: UUID; browser_handle: UUID; tab_id: string; frame_id: string;
  origin: string; top_origin: string; is_main_frame: boolean; expires_in_seconds: number;
}
export interface FillField { css: string; credential_ref: CredentialRef; credential_field: string }
export interface SecureFill {
  /** Create once, retain before calling, and poll this ID after any uncertainty. */
  operation_id: UUID; browser_handle: UUID; target_handle: UUID; fields: readonly FillField[];
}
export interface PromptFillField { css: string; field_name: string }
export interface SecurePromptFill {
  /** Field names/locators only. The human enters values in the native UI. */
  operation_id: UUID; browser_handle: UUID; target_handle: UUID; fields: readonly PromptFillField[];
}
export type FillState = 'pending' | 'filling' | 'filled' | 'denied' | 'cancelled'
  | 'expired' | 'failed' | 'partial' | 'uncertain';
export interface FillStatus {
  operation_id: UUID; state: FillState;
  fields: ('filled' | 'not_filled' | 'uncertain')[]; error: ErrorCode | null;
}
export interface DeliveryProfile { profile_id: UUID; label: string; kind: 'process' | 'http' }
export interface SecureDelivery { operation_id: UUID; profile_id: UUID }
export type DeliveryState = 'pending' | 'running' | 'completed' | 'denied' | 'cancelled' | 'expired' | 'failed' | 'uncertain';
export interface DeliveryStatus {
  operation_id: UUID; kind: 'process' | 'http'; state: DeliveryState;
  may_have_run: boolean; error: ErrorCode | null;
}
export function createOperationId(): UUID;
export class MagicVault {
  constructor(options?: MagicVaultOptions);
  status(options?: CallOptions): Promise<ServiceStatus>;
  listCredentials(options?: CallOptions): Promise<CredentialMetadata[]>;
  /** Requests metadata visibility only; cannot grant future effects or reveal values. */
  requestApproval(credentialRef: CredentialRef, options?: CallOptions): Promise<ApprovalStatus>;
  approvalStatus(approvalId: UUID, options?: CallOptions): Promise<ApprovalStatus>;
  listBrowsers(options?: CallOptions): Promise<BrowserInfo[]>;
  browserTargets(query: BrowserTargetsQuery, options?: CallOptions): Promise<BrowserTarget[]>;
  /** One submission, never retried. Does not submit a form or prove successful login. */
  secureFill(request: SecureFill, options?: CallOptions): Promise<FillStatus>;
  /** Native one-time input and approval; never saves or returns credential values. */
  securePromptFill(request: SecurePromptFill, options?: CallOptions): Promise<FillStatus>;
  fillStatus(operationId: UUID, options?: CallOptions): Promise<FillStatus>;
  cancelFill(operationId: UUID, options?: CallOptions): Promise<FillStatus>;
  /** Returns any terminal receipt, including denied/partial/uncertain; inspect state. */
  waitForFill(operationId: UUID, options?: WaitOptions): Promise<FillStatus>;
  listDeliveryProfiles(options?: CallOptions): Promise<DeliveryProfile[]>;
  /** Fixed, human-registered destination. Returns a receipt, never response content. */
  secureNewHttp(request: SecureDelivery, options?: CallOptions): Promise<DeliveryStatus>;
  /** Fixed, human-registered command. Returns a receipt, never stdout/stderr. */
  secureNewProcess(request: SecureDelivery, options?: CallOptions): Promise<DeliveryStatus>;
  deliveryStatus(operationId: UUID, options?: CallOptions): Promise<DeliveryStatus>;
  cancelDelivery(operationId: UUID, options?: CallOptions): Promise<DeliveryStatus>;
  /** Returns any terminal receipt, including denied/uncertain; inspect state. */
  waitForDelivery(operationId: UUID, options?: WaitOptions): Promise<DeliveryStatus>;
}
