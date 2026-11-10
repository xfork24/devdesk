type Installation = {
  key?: { os?: string };
  artifact?: { kind?: string };
  execution_supported?: boolean;
};

export function isProjectRuntimeInstallation(runtime: Installation) {
  return (
    runtime.execution_supported !== false &&
    runtime.key?.os === "linux" &&
    runtime.artifact?.kind === "docker_image"
  );
}
