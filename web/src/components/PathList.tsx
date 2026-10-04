/**
 * A list of paths on this computer, each with an optional note after it,
 * such as why it could not be deleted.
 */
export default function PathList({ paths }: { paths: readonly { path: string; note?: string }[] }) {
  return (
    <ul className="mt-2 list-disc pl-5 text-[0.813rem] text-text">
      {paths.map(({ path, note }) => (
        <li key={path} className="break-all">
          <code className="font-mono text-[0.75rem]">{path}</code>
          {note ? `: ${note}` : null}
        </li>
      ))}
    </ul>
  );
}
