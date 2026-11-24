import {
  createContext,
  useContext,
  useLayoutEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import { Layers3 } from "lucide-react";
import { version } from "../package.json";

const ExtensionLayout = createContext<{
  outlet: HTMLElement | null;
  content: HTMLElement | null;
  settings?: ReactNode;
  setMounted: (mounted: boolean) => void;
} | null>(null);

export function WorkbenchLayout({
  navigation,
  fallbackNavigation,
  domainSettings,
  children,
  overlay,
}: {
  navigation: ReactNode;
  fallbackNavigation?: ReactNode;
  domainSettings?: ReactNode;
  children: ReactNode;
  overlay?: ReactNode;
}) {
  const [outlet, setOutlet] = useState<HTMLDivElement | null>(null);
  const [content, setContent] = useState<HTMLDivElement | null>(null);
  const [mounted, setMounted] = useState(false);
  const extension = useMemo(
    () => ({ outlet, content, settings: domainSettings, setMounted }),
    [outlet, content, domainSettings],
  );
  return (
    <div className="app">
      <aside className="sidebar">
        <div className="brand">
          <Layers3 size={26} />
          <strong>DevDesk</strong>
          <small className="muted">v{version}</small>
        </div>
        {navigation}
        {!mounted && fallbackNavigation}
        <div ref={setOutlet} className="extension-navigation" />
      </aside>
      <div ref={setContent} className="main">
        <ExtensionLayout.Provider value={extension}>
          {children}
        </ExtensionLayout.Provider>
      </div>
      {overlay}
    </div>
  );
}

export function DomainLayout({
  navigation,
  children,
  overlay,
}: {
  navigation: ReactNode;
  children: ReactNode;
  overlay?: ReactNode;
}) {
  const extension = useContext(ExtensionLayout);
  const outlet = extension?.outlet;
  const setMounted = extension?.setMounted;
  const [settingsOutlet, setSettingsOutlet] = useState<HTMLElement | null>(
    null,
  );
  useLayoutEffect(() => {
    if (!outlet || !setMounted) return;
    setMounted(true);
    return () => setMounted(false);
  }, [outlet, setMounted]);
  useLayoutEffect(() => {
    // Installed packages mark their own settings page before the layout effect runs.
    setSettingsOutlet(
      extension?.settings
        ? (extension.content?.querySelector<HTMLElement>(
            ":scope > main.settings-page",
          ) ?? null)
        : null,
    );
  }, [children, extension]);
  return (
    <>
      {outlet && createPortal(navigation, outlet)}
      {children}
      {settingsOutlet && createPortal(extension?.settings, settingsOutlet)}
      {overlay}
    </>
  );
}
