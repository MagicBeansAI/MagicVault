import { MagicVault, MagicVaultError, createOperationId, type DeliveryStatus, type FillStatus } from '../../npm/sdk.cjs';

const vault = new MagicVault({ profile: 'agent' });
const operation_id = createOperationId();
const delivery: Promise<DeliveryStatus> = vault.secureNewHttp({ operation_id, profile_id: createOperationId() });
const fill: Promise<FillStatus> = vault.waitForFill(operation_id, { timeoutMs: 1000 });
void delivery; void fill;
const error = new MagicVaultError('transport_uncertain', operation_id);
const errorId: string | undefined = error.operationId;
void errorId;
// @ts-expect-error The SDK has no raw credential getter.
vault.getSecret('demo');
// @ts-expect-error A caller cannot override a registered destination.
vault.secureNewHttp({ operation_id, profile_id: createOperationId(), url: 'https://example.com' });
// @ts-expect-error A fill takes references only.
vault.secureFill({ operation_id, browser_handle: 'id', target_handle: 'id', fields: [{ css: '#password', value: 'secret' }] });
// @ts-expect-error Receipt-only API exposes no response body.
delivery.then(receipt => receipt.body);

const prompted: Promise<FillStatus> = vault.securePromptFill({ operation_id,
  browser_handle: 'id', target_handle: 'id', fields: [{css: '#password', field_name: 'password'}] });
void prompted;
// @ts-expect-error One-time values belong in native UI, never SDK arguments.
vault.securePromptFill({ operation_id, browser_handle: 'id', target_handle: 'id', fields: [{ css: '#password', field_name: 'password', value: 'secret' }] });
