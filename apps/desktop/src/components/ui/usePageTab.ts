import { useAppStore, type Page } from "../../store/app";

/**
 * The selected tab of `page`, kept in the store so returning to a page (or
 * being sent to it from the tray) lands on the same tab.
 */
export function usePageTab(
  page: Page,
  ids: readonly string[],
): [string, (id: string) => void] {
  const stored = useAppStore((state) => state.pageTabs[page]);
  const setPageTab = useAppStore((state) => state.setPageTab);
  const current = stored && ids.includes(stored) ? stored : (ids[0] ?? "");
  return [current, (id) => setPageTab(page, id)];
}
