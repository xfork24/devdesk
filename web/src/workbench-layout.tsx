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
  projectAction?: (project: string) => ReactNode;
  setMounted: (mounted: boolean) => void;
} | null>(null);

export function WorkbenchLayout({
  navigation,
  fallbackNavigation,
  domainSettings,
  domainProjectAction,
  children,
  overlay,
}: {
  navigation: ReactNode;
  fallbackNavigation?: ReactNode;
  domainSettings?: ReactNode;
  domainProjectAction?: (project: string) => ReactNode;
  children: ReactNode;
  overlay?: ReactNode;
}) {
  const [outlet, setOutlet] = useState<HTMLDivElement | null>(null);
  const [content, setContent] = useState<HTMLDivElement | null>(null);
  const [mounted, setMounted] = useState(false);
  const extension = useMemo(
    () => ({
      outlet,
      content,
      settings: domainSettings,
      projectAction: domainProjectAction,
      setMounted,
    }),
    [outlet, content, domainSettings, domainProjectAction],
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
  const [projectOutlet, setProjectOutlet] = useState<{
    element: HTMLElement;
    project: string;
  } | null>(null);
  useLayoutEffect(() => {
    if (!outlet || !setMounted) return;
    setMounted(true);
    return () => setMounted(false);
  }, [outlet, setMounted]);
  useLayoutEffect(() => {
    const settingsPage = extension?.settings
      ? extension.content?.querySelector(":scope > main.settings-page")
      : null;
    const panel =
      settingsPage?.querySelector<HTMLElement>(
        ":scope > [data-domain-storage-outlet]",
      ) ??
      // Installed packages identify this panel through their connection section.
      settingsPage?.querySelector<HTMLElement>(
        ":scope > .panel:has(> .kernel-connection)",
      );
    const storageRow = panel?.querySelector(
      ":scope > [data-domain-storage-row]",
    );
    // A project selection can add rows after the storage portal was mounted.
    if (panel && storageRow && storageRow !== panel.lastElementChild) {
      panel.append(storageRow);
    }
    setSettingsOutlet(panel ?? null);
  }, [children, extension]);
  useLayoutEffect(() => {
    const content = extension?.content;
    const project = outlet?.querySelector<HTMLSelectElement>(
      ".project-selector select",
    )?.value;
    const tab = outlet?.querySelector<HTMLButtonElement>(
      'nav button[aria-current="page"]',
    );
    if (
      !content ||
      !extension.projectAction ||
      !project ||
      (tab?.dataset.workbenchTab !== "projects" &&
        tab?.textContent?.trim() !== "项目管理")
    ) {
      setProjectOutlet(null);
      return;
    }
    const update = () => {
      // Older installed packages expose the same form without the SDK marker.
      const save =
        content.querySelector("[data-domain-project-save]") ??
        Array.from(content.querySelectorAll("form button")).find(
          (button) => button.textContent?.trim() === "保存项目配置",
        );
      const element = save?.closest<HTMLElement>(".action-control");
      setProjectOutlet((previous) =>
        element
          ? previous?.element === element && previous.project === project
            ? previous
            : { element, project }
          : null,
      );
    };
    const observer = new MutationObserver(update);
    observer.observe(content, { childList: true, subtree: true });
    update();
    return () => observer.disconnect();
  }, [navigation, extension, outlet]);
  return (
    <>
      {outlet && createPortal(navigation, outlet)}
      {projectOutlet &&
        createPortal(
          extension?.projectAction?.(projectOutlet.project),
          projectOutlet.element,
          projectOutlet.project,
        )}
      {children}
      {settingsOutlet && createPortal(extension?.settings, settingsOutlet)}
      {overlay}
    </>
  );
}
