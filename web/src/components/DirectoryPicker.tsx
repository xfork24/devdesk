import { useEffect, useRef, useState } from "react";
import { Folder, ArrowUp } from "lucide-react";

type Directory = { name: string; path: string };
type Listing = {
  path: string;
  parent: string | null;
  directories: Directory[];
  locations: Directory[];
};

export function DirectoryPicker({
  request,
  initialPath,
  onSelect,
  onClose,
}: {
  request: (
    method: string,
    params?: Record<string, unknown>,
  ) => Promise<unknown>;
  initialPath: string;
  onSelect: (path: string) => Promise<void>;
  onClose: () => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const sequence = useRef(0);
  const [listing, setListing] = useState<Listing>();
  const [loading, setLoading] = useState(true);
  const [selecting, setSelecting] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    dialog.current?.showModal();
    let active = true;
    request("permission.source.browse", { path: initialPath })
      .then((value) => {
        if (active) setListing(value as Listing);
      })
      .catch((e: unknown) => {
        if (active) setError(e instanceof Error ? e.message : String(e));
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
      sequence.current += 1;
    };
  }, [request, initialPath]);

  async function navigate(path?: string) {
    const current = ++sequence.current;
    setLoading(true);
    setError("");
    try {
      const value = await request(
        "permission.source.browse",
        path ? { path } : {},
      );
      if (current === sequence.current) setListing(value as Listing);
    } catch (e) {
      if (current === sequence.current)
        setError(e instanceof Error ? e.message : String(e));
    } finally {
      if (current === sequence.current) setLoading(false);
    }
  }

  async function select() {
    if (!listing || loading || selecting) return;
    setSelecting(true);
    setError("");
    try {
      await onSelect(listing.path);
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setSelecting(false);
    }
  }

  return (
    <dialog
      ref={dialog}
      className="directory-picker"
      aria-labelledby="directory-picker-title"
      onCancel={(event) => {
        event.preventDefault();
        if (!selecting) onClose();
      }}
    >
      <h3 id="directory-picker-title">选择预设文件夹</h3>
      <p className="muted">选择内核所在机器上的文件夹，作为唯一预设来源。</p>
      <fieldset disabled={loading || selecting}>
        <legend>文件夹</legend>
        <nav className="directory-picker-locations" aria-label="常用文件夹">
          {(listing?.locations ?? []).map((location) => (
            <button
              type="button"
              key={location.name}
              onClick={() => void navigate(location.path)}
            >
              {location.name}
            </button>
          ))}
          {!listing ? (
            <button type="button" onClick={() => void navigate()}>
              数据目录
            </button>
          ) : null}
        </nav>
        <p className="directory-picker-path" aria-label="正在浏览的文件夹">
          {listing?.path || initialPath}
        </p>
        <button
          type="button"
          disabled={!listing?.parent}
          onClick={() => void navigate(listing?.parent ?? undefined)}
        >
          <ArrowUp size={16} aria-hidden="true" /> 上一级文件夹
        </button>
        <ul
          className="directory-picker-list"
          aria-label="子文件夹"
          aria-busy={loading}
        >
          {listing?.directories.map((directory) => (
            <li key={directory.path}>
              <button
                type="button"
                onClick={() => void navigate(directory.path)}
              >
                <Folder size={18} aria-hidden="true" /> {directory.name}
              </button>
            </li>
          ))}
        </ul>
        {listing?.directories.length === 0 ? (
          <p className="muted">没有子文件夹，可选择当前文件夹。</p>
        ) : null}
      </fieldset>
      {loading ? <p role="status">正在读取文件夹…</p> : null}
      {error ? <p role="alert">{error}</p> : null}
      <div className="actions">
        <button type="button" disabled={selecting} onClick={onClose}>
          取消
        </button>
        <button
          type="button"
          disabled={!listing || loading || selecting}
          onClick={() => void select()}
        >
          {selecting ? "正在切换…" : "选择此文件夹"}
        </button>
      </div>
    </dialog>
  );
}
