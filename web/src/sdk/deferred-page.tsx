import { useEffect, useState, type ComponentType } from "react";

export function deferPage<Props extends object>(
  importPage: () => Promise<ComponentType<Props>>,
) {
  let loaded: ComponentType<Props> | undefined;
  let pending: Promise<ComponentType<Props>> | undefined;

  function preload() {
    return (pending ??= importPage().then(
      (component) => {
        loaded = component;
        return component;
      },
      (error: unknown) => {
        pending = undefined;
        throw error;
      },
    ));
  }

  function Page(props: Props) {
    const [Content, setContent] = useState<ComponentType<Props> | undefined>(
      () => loaded,
    );
    const [failed, setFailed] = useState(false);
    useEffect(() => {
      if (Content) return;
      let active = true;
      void preload().then(
        (component) => {
          if (active) setContent(() => component);
        },
        () => {
          if (active) setFailed(true);
        },
      );
      return () => {
        active = false;
      };
    }, [Content]);

    if (Content) return <Content {...props} />;
    if (failed) return <p role="alert">页面载入失败，请重新载入。</p>;
    return (
      <p className="muted" role="status">
        正在载入页面…
      </p>
    );
  }

  return { Page, preload };
}
