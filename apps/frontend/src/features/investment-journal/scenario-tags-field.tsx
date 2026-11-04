import { Button } from "@wealthfolio/ui";
import { Input } from "@wealthfolio/ui/components/ui/input";
import { Field } from "./controls";
import { useJournalLocale } from "./locale-context";

export function ScenarioTagsField({
  tags,
  setTags,
  suggestions,
}: {
  tags: string;
  setTags: (value: string) => void;
  suggestions: string[];
}) {
  const { t } = useJournalLocale();
  const lastTagStart = Math.max(tags.lastIndexOf(","), tags.lastIndexOf("，")) + 1;
  const tagPrefix =
    tags.slice(0, lastTagStart) + (/^\s*/.exec(tags.slice(lastTagStart))?.[0] ?? "");
  const activeTag = tags.slice(lastTagStart).trim().toLocaleLowerCase();
  const earlierTags = new Set(
    tags
      .slice(0, lastTagStart)
      .split(/[,，]/)
      .map((tag) => tag.trim()),
  );
  const matchingTags = suggestions
    .filter(
      (tag) =>
        !earlierTags.has(tag) &&
        tag.toLocaleLowerCase() !== activeTag &&
        tag.toLocaleLowerCase().startsWith(activeTag),
    )
    .slice(0, 8);
  return (
    <Field label={t("scenario_tags")} hint={t("scenario_tags_hint")}>
      {(id) => (
        <>
          <Input id={id} value={tags} onChange={(event) => setTags(event.target.value)} />
          {matchingTags.length > 0 && (
            <div className="flex flex-wrap gap-2">
              {matchingTags.map((tag) => (
                <Button
                  key={tag}
                  type="button"
                  variant="outline"
                  size="sm"
                  className="h-auto min-h-8 max-w-full whitespace-normal [overflow-wrap:anywhere]"
                  onClick={() => setTags(tagPrefix + tag)}
                >
                  {tag}
                </Button>
              ))}
            </div>
          )}
        </>
      )}
    </Field>
  );
}
