import { Button } from "@xcss/web/admin-ui";
import { t } from "@xcss/web/admin-ui/i18n";
import type { PageCursors } from "./api";

export function PageNavigation({ page, loading, first, move, label }: {
  page: PageCursors | null;
  loading: boolean;
  first(): void;
  move(cursor: string): void;
  label: string;
}) {
  return <nav className="xcss-actions" aria-label={label}>
    <Button disabled={loading || !page || page.previous_cursor === null} onClick={first}>{t("首页", "First page")}</Button>
    <Button disabled={loading || !page || page.previous_cursor === null} onClick={() => { if (page?.previous_cursor) move(page.previous_cursor); }}>{t("上一页", "Previous page")}</Button>
    <Button disabled={loading || !page || page.next_cursor === null} onClick={() => { if (page?.next_cursor) move(page.next_cursor); }}>{t("下一页", "Next page")}</Button>
  </nav>;
}
