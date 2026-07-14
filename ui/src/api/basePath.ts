export interface GoldeneyeUiRuntimeConfig {
  apiBasePath?: string;
}

declare global {
  var __GOLDENEYE_UI_CONFIG__: GoldeneyeUiRuntimeConfig | undefined;
}

const INVALID_PATH = /[\\?#\u0000-\u001f\u007f]/;

export function normalizeApiBasePath(value: string | undefined): string {
  const raw = (value ?? "").trim();
  if (!raw || raw === "/") return "";
  if (/^[A-Za-z][A-Za-z0-9+.-]*:/.test(raw) || INVALID_PATH.test(raw)) {
    throw new Error(`invalid API base path: ${raw}`);
  }
  const path = raw.startsWith("/") ? raw : `/${raw}`;
  if (path.split("/").some((segment) => segment === "." || segment === "..")) {
    throw new Error(`invalid API base path: ${raw}`);
  }
  return path.replace(/\/+$/, "");
}

export function configuredApiBasePath(): string {
  const runtime = globalThis.__GOLDENEYE_UI_CONFIG__?.apiBasePath;
  if (runtime !== undefined) return normalizeApiBasePath(runtime);

  if (typeof document !== "undefined") {
    const meta = document.querySelector<HTMLMetaElement>(
      'meta[name="goldeneye-api-base"]',
    );
    if (meta?.content) return normalizeApiBasePath(meta.content);
  }

  return normalizeApiBasePath(import.meta.env.VITE_API_BASE_PATH);
}

export function apiUrl(route: string, basePath = configuredApiBasePath()): string {
  const routePath = route.split("?")[0];
  if (
    !route.startsWith("/") ||
    (!route.startsWith("/api/") && route !== "/rpc") ||
    INVALID_PATH.test(routePath) ||
    routePath.split("/").some((segment) => segment === "." || segment === "..")
  ) {
    throw new Error(`invalid Goldeneye API route: ${route}`);
  }
  return `${normalizeApiBasePath(basePath)}${route}`;
}
