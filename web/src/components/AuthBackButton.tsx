import Button from "./Button";
import { BackChevronIcon } from "./icons";

export default function AuthBackButton({ onClick, label }: { onClick: () => void; label: string }) {
  return (
    <Button variant="ghost" onPress={onClick} className="-ml-2 gap-1.5">
      <BackChevronIcon size={10} className="" />
      {label}
    </Button>
  );
}
