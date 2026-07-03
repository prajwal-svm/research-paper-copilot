# sync-remotes

## Purpose

Bring-your-own dumb-blob storage backends (free-tier R2 recommended, self-hosted MinIO first-class, plain folders) and account-less device pairing.

## Requirements

### Requirement: Bring-your-own dumb-blob remotes, free by default
Sync SHALL work against user-provided storage through a minimal blob interface — S3-compatible endpoints and plain local folders (enabling iCloud Drive/Dropbox/Syncthing/USB as transports) — with no first-party server and no account with us. **The recommended documented path SHALL be a free-tier Cloudflare R2 bucket** (zero infrastructure, ciphertext-only storage), and **self-hosted S3 (MinIO on the user's own server, e.g. via Coolify) SHALL be an equally supported first-class target** (custom endpoints, path-style addressing); nothing SHALL require a paid cloud account. Remote credentials SHALL live in the OS keychain, never in bundles or config files. Configuring a remote SHALL show exactly what will be stored there and that it is ciphertext.

#### Scenario: Free R2 bucket as the default path
- **WHEN** the user follows the recommended setup with an R2 free-tier bucket's endpoint and access keys
- **THEN** sync works end-to-end at no cost, the egress disclosure names the R2 host, and the bucket contains only encrypted blobs

#### Scenario: Self-hosted MinIO on the user's server
- **WHEN** the user points the S3 backend at their own MinIO endpoint (self-signed or LAN/VPN address) with its access keys
- **THEN** sync works end-to-end against their hardware, with the endpoint host shown in the egress disclosure and nothing sent anywhere else

#### Scenario: Folder remote via existing cloud drive
- **WHEN** the user points the folder backend at an iCloud Drive directory
- **THEN** sync works with no further setup, and the stored objects are encrypted blobs

#### Scenario: Credentials in the keychain
- **WHEN** an S3 remote is configured
- **THEN** the access keys are stored in the OS keychain and are absent from every file the app writes

### Requirement: Account-less device pairing
A second device SHALL join a library by providing the same remote configuration and library passphrase — nothing else. There SHALL be no identity registration; the passphrase-derived key both decrypts and proves membership. A wrong passphrase SHALL fail cleanly with no partial state.

#### Scenario: New laptop
- **WHEN** the user configures the same bucket and passphrase on a new machine
- **THEN** the library pulls down complete (with heavy caches rebuilding locally) and both devices sync thereafter

#### Scenario: Wrong passphrase
- **WHEN** the passphrase is mistyped on join
- **THEN** decryption fails with a plain-language message and nothing partial is written to the library
