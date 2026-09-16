import sdk = require('../../npm/sdk.cjs');
const vault = new sdk.MagicVault({ profile: 'agent' });
const result: Promise<sdk.ServiceStatus> = vault.status();
void result;
