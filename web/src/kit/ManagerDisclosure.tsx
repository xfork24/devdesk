import {
  useContext,
  useEffect,
  useRef,
  type ComponentPropsWithoutRef,
} from "react";
import { ManagerDisclosureContext } from "./manager-disclosure-state";

export function ManagerDisclosure({
  open = false,
  ...props
}: ComponentPropsWithoutRef<"details">) {
  const revision = useContext(ManagerDisclosureContext)?.revision;
  const element = useRef<HTMLDetailsElement>(null);
  useEffect(() => {
    if (element.current) element.current.open = open;
  }, [revision, open]);
  return <details {...props} ref={element} open={open} />;
}
