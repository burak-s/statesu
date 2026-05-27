/** @type {import('tailwindcss').Config} */
module.exports = {
    content: ["./internal/view/templates/**/*.html"],
    theme: {
        extend: {
            fontFamily: {
                sans: ["Inter", "system-ui", "sans-serif"],
            },
            colors: {
                brand: {
                    50: "#eaf3f5",
                    100: "#d5e7eb",
                    200: "#abcfd7",
                    300: "#80b7c3",
                    400: "#569faf",
                    500: "#307484",
                    600: "#26606d",
                    700: "#1d4b54",
                    800: "#13353c",
                    900: "#0a1f24",
                },
            },
        },
    },
    plugins: [],
};
