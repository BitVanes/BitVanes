# @bitvanes/web

The web surface for BitVanes, the zero-trust local PII purification engine. It
serves two roles from one Vite + React + TypeScript codebase:

1. **Public landing page** (`bitvanes.com`) — marketing site for compliance,
   legal, and ops teams.
2. **Local dashboard** — a browser UI that talks **only** to the local BitVanes
   daemon at `http://127.0.0.1:8080`. No data ever leaves the user's machine.

> The browser no longer runs the engine. The previous in-browser processing
> architecture was removed in the purification rebrand (multi-gigabyte document
> streams and OCR are a poor fit for the browser sandbox). All processing is
> done by the native engine via the local daemon.

## Architecture

- **Framework:** Vite 6 + TypeScript + React 19.
- **Styling:** hand-written CSS with a design-token system (`src/index.css`) —
  not Tailwind.
- **Processing:** none in-browser. The dashboard POSTs raw text to the local
  daemon's `/filter` endpoint and renders the redacted result.

## Getting started

```bash
npm install
npm run dev      # landing page + dashboard UI
```

To use the dashboard, start the local daemon in another terminal:

```bash
# from the cli repo
bitvanes daemon --port 8080 --rules email,ssn,credit_card,phone,street_address
```

Then open the dashboard view and click **Scrub locally**.

## Build

```bash
npm run build    # tsc -b && vite build
npm run preview
```

The built bundle is deployed to Vercel for `bitvanes.com` and may also be
served directly by the daemon (`bitvanes daemon --dashboard-dir ./dist`).
