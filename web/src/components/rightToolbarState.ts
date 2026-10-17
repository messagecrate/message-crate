import { createContext, type ReactNode } from "react";

type RightToolbarContextValue = {
  toolbar: ReactNode;
  setToolbar: (node: ReactNode | null) => void;
};

export const RightToolbarContext = createContext<RightToolbarContextValue>({
  toolbar: null,
  setToolbar: () => {},
});
