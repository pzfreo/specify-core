# Handing two STEP files to /diff

Another web page (a CAD tool's viewer, say) can open draftwright's STEP diff with two files it
already has. The files go from that page to /diff inside the person's browser: they are not
uploaded, and nothing is fetched from your server.

1. Open `https://editor.draftwright.io/diff` in a new window, from a click (a pop-up blocker
   lets that through). Don't pass `noopener`: /diff answers the page that opened it.
2. /diff posts `{ type: 'draftwright:diff-ready' }` to that page when it is listening.
3. Answer with the two files:

```js
function compareInDraftwright(before, after) {
  // before, after: { name: 'bracket.step', data: Blob | File | ArrayBuffer | Uint8Array }
  const origin = 'https://editor.draftwright.io';
  const diff = window.open(`${origin}/diff`);
  addEventListener('message', function ready(e) {
    if (e.source !== diff || e.origin !== origin || e.data?.type !== 'draftwright:diff-ready') return;
    removeEventListener('message', ready);
    diff.postMessage({ type: 'draftwright:diff', before, after }, origin);
  });
}
```

Each file is a STEP file (`.step`, `.stp`) or a gzipped one. Only the name's last part is shown.
The page says which site sent them ("Sent by example.com"), and compares them as if the person
had picked them: the report, the part coloured by what changed, and each line linked to its
faces.

/diff listens only to the window that opened it. It cannot be put in an iframe.
