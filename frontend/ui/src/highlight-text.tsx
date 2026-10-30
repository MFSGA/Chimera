type HighlightTextProps =
  | {
      searchText: string;
      className?: string;
      children: string;
      search?: never;
      text?: never;
    }
  | {
      search: string;
      text: string;
      className?: string;
      searchText?: never;
      children?: never;
    };

/** Highlight every case-insensitive occurrence of a search term. */
export default function HighlightText(props: HighlightTextProps) {
  const searchText = props.searchText ?? props.search;
  const children = props.children ?? props.text;
  const { className } = props;

  if (!searchText.trim()) {
    return <span className={className}>{children}</span>;
  }

  const parts: { text: string; isHighlight: boolean }[] = [];
  const searchLower = searchText.toLowerCase();
  const textLower = children.toLowerCase();

  let lastIndex = 0;
  let index = textLower.indexOf(searchLower, lastIndex);

  while (index !== -1) {
    if (index > lastIndex) {
      parts.push({
        text: children.slice(lastIndex, index),
        isHighlight: false,
      });
    }

    parts.push({
      text: children.slice(index, index + searchText.length),
      isHighlight: true,
    });

    lastIndex = index + searchText.length;
    index = textLower.indexOf(searchLower, lastIndex);
  }

  if (lastIndex < children.length) {
    parts.push({
      text: children.slice(lastIndex),
      isHighlight: false,
    });
  }

  return (
    <span className={className}>
      {parts.map((part, index) =>
        part.isHighlight ? (
          <mark
            key={index}
            className="rounded bg-yellow-400 px-0.5 text-black dark:bg-yellow-500"
          >
            {part.text}
          </mark>
        ) : (
          <span key={index}>{part.text}</span>
        ),
      )}
    </span>
  );
}
