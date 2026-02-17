import { useState, useEffect } from "react";

const STORAGE_KEY = "theme";

export function useTheme() {
    const [isDark, setIsDark] = useState(() => {
        return localStorage.getItem(STORAGE_KEY) === "dark";
    });

    useEffect(() => {
        const root = document.documentElement;
        if (isDark) {
            root.classList.add("dark");
        } else {
            root.classList.remove("dark");
        }
        localStorage.setItem(STORAGE_KEY, isDark ? "dark" : "light");
    }, [isDark]);

    const toggle = () => setIsDark((prev) => !prev);

    return { isDark, toggle };
}
