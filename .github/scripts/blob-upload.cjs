// Publish the Windows installer to the Noren project's Vercel Blob store.
//
// Invoked by .github/workflows/ci.yml (Windows job, push to main only) as:
//   node .github/scripts/blob-upload.cjs <path-to-Setup.exe>
//
// Auth is the BLOB_READ_WRITE_TOKEN env var (a GitHub Actions secret holding the
// read-write token of the Noren Vercel Blob store). The token alone identifies the
// store — no project/org id needed. A fixed pathname + allowOverwrite keeps the public
// URL stable across releases, so AGENT_SETUP_URL on the Noren side is set once.
//
// On success prints `AGENT_SETUP_URL=<url>` — copy that value into the Noren env the
// first time; it does not change afterwards.

const fs = require('fs');
const { put } = require('@vercel/blob');

const file = process.argv[2];
if (!file) {
	console.error('usage: node blob-upload.cjs <path-to-Setup.exe>');
	process.exit(1);
}

put('brevly-print/BrevlyPrint-Setup.exe', fs.readFileSync(file), {
	access: 'public',
	addRandomSuffix: false, // stable path -> stable public URL
	allowOverwrite: true, // replace the previous release's installer
	contentType: 'application/octet-stream', // force a download of the .exe
	token: process.env.BLOB_READ_WRITE_TOKEN,
})
	.then((r) => console.log('AGENT_SETUP_URL=' + r.url))
	.catch((e) => {
		console.error(e);
		process.exit(1);
	});
