# Desktop shell

The desktop app is intentionally deferred until the first headless V3 vertical slice is green.

When introduced, this app must remain a thin user-facing shell over `app-core`. Do not move domain validation, project persistence rules, or Arnis process logic into the GUI layer.

Planned first desktop responsibilities:

1. campus search;
2. campus selection and automatic project creation;
3. polygon boundary confirmation/editing;
4. minimal generation settings;
5. generation progress/preview;
6. open generated result.
