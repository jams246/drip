import { type ThemeName, themes } from './themes'
import { Label } from '../components/label'
import { RadioGroup, RadioGroupItem } from '../components/radio-group'

type ThemePickerProps = {
  theme: ThemeName
  onThemeChange: (theme: ThemeName) => void
}

export function ThemePicker({ theme, onThemeChange }: ThemePickerProps) {
  return (
    <fieldset className="theme-picker">
      <legend className="theme-picker__legend">Appearance</legend>
      <RadioGroup
        className="theme-picker__choices"
        value={theme}
        onValueChange={(next) => {
          const choice = themes.find((value) => value.name === next)
          if (choice) onThemeChange(choice.name)
        }}
        aria-label="Appearance"
      >
        {themes.map((choice) => (
          <Label className="theme-picker__choice" key={choice.name} htmlFor={`theme-${choice.name}`}>
            <RadioGroupItem className="theme-picker__radio" id={`theme-${choice.name}`} value={choice.name} aria-label={choice.label} />
            <span className="theme-picker__preview" data-theme={choice.name} aria-hidden="true">
              <span className="theme-picker__preview-line" />
              <span className="theme-picker__preview-line theme-picker__preview-line--short" />
              <span className="theme-picker__preview-bar" />
            </span>
            <span className="theme-picker__label">{choice.label}</span>
            <span className="theme-picker__mode">{choice.description}</span>
          </Label>
        ))}
      </RadioGroup>
    </fieldset>
  )
}
