import { useEffect, useState } from 'react';

const REPO = 'BitVanes/BitVanes';
const RELEASES_PAGE = 'https://github.com/BitVanes/BitVanes/releases';
const API = `https://api.github.com/repos/${REPO}/releases/latest`;

type Asset = { name: string; browser_download_url: string };
type Release = { tag_name: string; assets: Asset[]; html_url: string };

/** Detect the visitor's OS from the UA string. */
function detectOs(): 'mac' | 'windows' | 'linux' | 'other' {
  const ua = navigator.userAgent.toLowerCase();
  if (ua.includes('mac')) return 'mac';
  if (ua.includes('win')) return 'windows';
  if (ua.includes('linux')) return 'linux';
  return 'other';
}

/** Match an asset name to an OS key. */
function assetOs(name: string): 'mac' | 'windows' | 'linux' | null {
  const n = name.toLowerCase();
  if (n.includes('macos') || n.includes('darwin')) return 'mac';
  if (n.includes('windows') || n.endsWith('.zip') || n.includes('.msi')) return 'windows';
  if (n.includes('linux')) return 'linux';
  return null;
}

/**
 * OS-aware download buttons. Fetches the latest GitHub release and surfaces
 * the binary for the visitor's platform, plus the other two and the shell /
 * PowerShell one-liner installers.
 */
export default function DownloadButtons() {
  const [release, setRelease] = useState<Release | null>(null);
  const [err, setErr] = useState(false);
  const visitorOs = detectOs();

  useEffect(() => {
    fetch(API, { headers: { Accept: 'application/vnd.github+json' } })
      .then((r) => (r.ok ? r.json() : Promise.reject()))
      .then(setRelease)
      .catch(() => setErr(true));
  }, []);

  // Map OS → asset URL.
  const assetFor = (os: string) =>
    release?.assets.find((a) => assetOs(a.name) === os)?.browser_download_url;

  const primaryUrl = assetFor(visitorOs);
  const primaryLabel =
    visitorOs === 'mac' ? 'Download for macOS' : visitorOs === 'windows' ? 'Download for Windows' : visitorOs === 'linux' ? 'Download for Linux' : 'Download';

  if (err) {
    // Rate-limited or no release yet — fall back to the releases page.
    return (
      <div className="dl-buttons">
        <a className="btn-primary" href={RELEASES_PAGE} target="_blank" rel="noreferrer">
          {primaryLabel}
        </a>
        <p className="dl-foot">Browse all releases on GitHub →</p>
      </div>
    );
  }

  if (!release) {
    return (
      <div className="dl-buttons">
        <span className="dl-loading">Checking latest release…</span>
      </div>
    );
  }

  return (
    <div className="dl-buttons">
      <div className="dl-primary-row">
        {primaryUrl ? (
          <a className="btn-primary" href={primaryUrl}>
            {primaryLabel} <span className="dl-tag">{release.tag_name}</span>
          </a>
        ) : (
          <a className="btn-primary" href={release.html_url}>
            Download <span className="dl-tag">{release.tag_name}</span>
          </a>
        )}
      </div>
      <div className="dl-all-row">
        <span>Also:</span>
        {assetFor('mac') && (
          <a href={assetFor('mac')!}>macOS</a>
        )}
        {assetFor('windows') && (
          <a href={assetFor('windows')!}>Windows</a>
        )}
        {assetFor('linux') && (
          <a href={assetFor('linux')!}>Linux</a>
        )}
        <a href={release.html_url} target="_blank" rel="noreferrer">
          all releases
        </a>
      </div>
      <details className="dl-cli">
        <summary>Or install from your terminal</summary>
        <div className="dl-cli-blocks">
          <div>
            <span className="dl-cli-label">macOS / Linux</span>
            <pre>
              <code>curl -fsSL https://bitvanes.com/install.sh | sh</code>
            </pre>
          </div>
          <div>
            <span className="dl-cli-label">Windows (PowerShell)</span>
            <pre>
              <code>irm https://bitvanes.com/install.ps1 | iex</code>
            </pre>
          </div>
        </div>
        <p className="dl-unsigned-note">
          ⚠️ Binaries are currently unsigned — macOS will show “unidentified
          developer” (right-click → Open, or{' '}
          <code>xattr -d com.apple.quarantine /usr/local/bin/bitvanes</code>) and
          Windows SmartScreen will prompt (More info → Run anyway).
        </p>
      </details>
    </div>
  );
}
