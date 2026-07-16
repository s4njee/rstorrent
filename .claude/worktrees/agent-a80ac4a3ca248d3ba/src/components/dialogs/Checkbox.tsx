/**
 * Styled checkbox used across dialogs: a visible box (cyan when checked) with a
 * hidden native input for accessibility; clicking the label toggles it.
 */

import forms from "./forms.module.css";

interface CheckboxProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
  disabled?: boolean;
  title?: string;
}

export function Checkbox({
  checked,
  onChange,
  label,
  disabled,
  title,
}: CheckboxProps) {
  return (
    <label
      className={`${forms.check} ${checked ? "" : forms.off}`}
      title={title}
      style={disabled ? { opacity: 0.5 } : undefined}
    >
      <span className={`${forms.box} ${checked ? forms.checked : ""}`}>
        {checked ? "✓" : ""}
      </span>
      <input
        type="checkbox"
        className={forms.hidden}
        checked={checked}
        disabled={disabled}
        onChange={(e) => onChange(e.currentTarget.checked)}
      />
      {label}
    </label>
  );
}
