import { createContext, useContext, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { Layers3 } from "lucide-react";
import { version } from "../package.json";

const ExtensionNavigation = createContext<HTMLElement | null>(null);

export function WorkbenchLayout({
  navigation,
  children,
  overlay,
}: {
  navigation: ReactNode;
  children: ReactNode;
  overlay?: ReactNode;
}) {
  const [outlet, setOutlet] = useState<HTMLDivElement | null>(null);
  return (
    <div className="app">
      <aside className="sidebar">
        <div className="brand">
          <Layers3 size={26} />
          <strong>DevDesk</strong>
          <small className="muted">v{version}</small>
        </div>
        {navigation}
        <div ref={setOutlet} className="extension-navigation" />
      </aside>
      <div className="main">
        <ExtensionNavigation.Provider value={outlet}>
          {children}
        </ExtensionNavigation.Provider>
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
  const outlet = useContext(ExtensionNavigation);
  return (
    <>
      {outlet && createPortal(navigation, outlet)}
      {children}
      {overlay}
    </>
  );
}
