import { createContext, useContext, type ReactNode } from "react";
import { useAppController } from "../../use-app-controller";
import { RcodexMantineProvider } from "./mantine-theme";

const AppStateContext = createContext<ReturnType<typeof useAppController> | null>(
  null,
);

/**
 * @component AppStateProvider
 * @description 注入控制面状态，并包一层 Mantine 主题。
 */
export function AppStateProvider({ children }: { children: ReactNode }) {
  const value = useAppController();
  return (
    <AppStateContext.Provider value={value}>
      <RcodexMantineProvider colorScheme={value.resolvedTheme} accent={value.accent}>
        {children}
      </RcodexMantineProvider>
    </AppStateContext.Provider>
  );
}

/**
 * @function useAppState
 * @description 读取控制面共享状态。必须在 AppStateProvider 内使用。
 */
export function useAppState() {
  const value = useContext(AppStateContext);
  if (!value) {
    throw new Error("useAppState must be used within AppStateProvider");
  }
  return value;
}
