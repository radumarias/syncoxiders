# Retry and resumable downloads

Oxfer supports two different kinds of recovery. Neither runs a transfer while
the browser is closed or the operating system has suspended the page.

## Live reconnect

The receiver automatically retries transient connection failures with bounded
exponential backoff. The UI shows the attempt number; Cancel also cancels the
backoff or an in-progress connection attempt.

Every new connection repeats authentication and checks the complete original
manifest before requesting bytes. A reconnect retains:

- Open destinations and their acknowledged byte offsets.
- The running BLAKE3 state for the current file.
- Completed files, so they are not downloaded or published twice.

A reconnect may use the encrypted relay rather than negotiating WebRTC again.
This is separate from the existing WebRTC-to-relay fallback within a live
connection.

Authentication rejection, incompatible or changed manifests, protocol errors,
hash mismatches, and local destination failures are not transient network
errors. They must fail rather than loop. In particular, a cancelled or timed-out
destination write has an uncertain commit state and must not be replayed.

After the retry budget is exhausted, ordinary destinations are cleaned up.
Opt-in resumable browser copies remain available locally.

## Recovery after closing the browser

Before receiving, select **Keep a resumable copy on this device**, then
**Receive / resume local copy**. This is opt-in because it retains plaintext
file data and metadata in this browser's private site storage. It is not an
encrypted local vault.

1. The receiver stages files in the origin-private file system (OPFS).
2. Each acknowledged checkpoint represents flushed file data and committed
   metadata, not merely bytes queued for writing.
3. When the receiver returns, **Copies on this device** lists retained progress.
4. Reopen/paste a sender link and select the resumable-copy option again. The
   storage key covers the complete manifest: names, sizes, BLAKE3 hashes and
   ordering. A matching copy resumes; a different manifest is a separate copy.
5. Recovery reads the checkpointed prefix in bounded chunks to reconstruct its
   BLAKE3 state. Excess uncheckpointed bytes are not trusted.
6. Only a file whose full hash matches the sender's manifest is marked verified.
   Use **Download copy** to export it to the browser's normal Downloads/Files
   flow. The local copy remains until explicitly deleted.

If the sender stayed open, use the original private link. If the sender also
closed the tab, they must reselect and rehash the same files, start sharing,
and send a fresh link. Oxfer does not retain sender file-picker handles,
private endpoint keys, or a copy of the sender's files.

**Tickets and bearer capabilities are not persisted.** The URL fragment is
still scrubbed after accepting a link. Retained file metadata is not sufficient
to reconnect without a valid link.

### Browser and storage limits

- A secure context and OPFS synchronous access handles in a dedicated worker
  are required. Unsupported browsers show an error; uncheck the option to use
  the ordinary download routes. There is no silent fallback that claims a
  non-resumable download is durable.
- The worker uses `createSyncAccessHandle`, rather than requiring
  `createWritable`, to support older Safari/iOS implementations. Feature
  availability, storage quota, and actual device behavior still matter.
- A browser can evict site data. Private-browsing cleanup, clearing website
  data, and changing origin also make retained copies unavailable. Storage
  persistence requests are best effort, not a guarantee.
- Staging consumes approximately the file size in local site storage. Exporting
  can require additional disk space for the destination copy.
- Exclusive file locks prevent two tabs from writing/deleting the same active
  copy. Close or cancel the other transfer before retrying.
- Cancelling a resumable receive preserves its checkpoints. **Delete local
  copy**, followed by confirmation, removes both progress and completed copies.
- File-backed Blob export avoids explicit whole-file buffering in Oxfer.
  Browser-internal buffering and the iOS Files/download UI are browser-owned;
  large-file export still requires real-device validation.
- Durable restart recovery is browser-only. Native receives benefit from live
  reconnect, but do not yet have a persisted-session library.

The existing File System Access, service-worker streaming and memory sinks
remain available when the resumable option is off. A browser-managed partial
download from those routes cannot be reopened by Oxfer after a page reload.

## Validation

Run the contributor gate and browser integration tests from `p2p-transfer/`:

```sh
./check.sh
wasm-pack test --headless --firefox -- --test webrtc_wasm
```

For an end-to-end browser/device check:

1. Share a large fixture and receive with local-copy retention enabled.
2. Interrupt the control connection after some bytes are committed. Verify the
   retry counter, restored progress, successful hash verification and a single
   completed receipt.
3. Repeat, but close the receiver tab while transferring. Reopen the same
   origin and link; confirm that progress is listed and the existing prefix is
   reused. Also test closing/reopening the sender and using its fresh link.
4. Export a verified copy and compare its bytes/hash with the fixture.
5. Attempt to open the same local copy in two receiving tabs. The second must
   fail safely rather than truncate or mix data.
6. Simulate denied storage/quota exhaustion. The UI must report the storage
   failure rather than silently switching to an ephemeral destination.
7. Delete a retained copy with confirmation and reload. It must remain deleted.

`#dev` bypasses app-shell caches and disables the service-worker download sink;
it does not delete OPFS resume data. Test service-worker downloads separately
without `#dev`. Do not include private share links in test logs or screenshots.
