import { type ThemeName, themes } from './themes'
import './themes.css'

type ThemePickerProps = {
  theme: ThemeName
  onThemeChange: (theme: ThemeName) => void
}

export function ThemePicker({ theme, onThemeChange }: ThemePickerProps) {
  return (
    <fieldset className="theme-picker">
      <legend className="theme-picker__legend">Appearance</legend>
      <div className="theme-picker__choices">
        {themes.map((choice) => (
          <label className="theme-picker__choice" key={choice.name}>
            <input
              className="theme-picker__radio"
              type="radio"
              name="theme"
              value={choice.name}
              checked={theme === choice.name}
              onChange={() => onThemeChange(choice.name)}
            />
            <span className="theme-picker__preview" data-theme={choice.name} aria-hidden="true">
              <span className="theme-picker__preview-line" />
              <span className="theme-picker__preview-line theme-picker__preview-line--short" />
              <span className="theme-picker__preview-bar" />
            </span>
            <span className="theme-picker__label">{choice.label}</span>
            <span className="theme-picker__mode">{choice.description}</span>
          </label>
        ))}
      </div>
    </fieldset>
  )
}
