import {
  forwardRef,
  useCallback,
  useEffect,
  useId,
  useImperativeHandle,
  useMemo,
  useState,
} from 'react';
import { useTranslation } from 'react-i18next';
import type { AskUserQuestionItem, QuestionAnswer } from 'shared/types';
import { Check, Loader2, MessageCircleQuestion } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';
import {
  formatApprovalTime,
  useApprovalCountdown,
} from '@/hooks/useApprovalCountdown';

export interface AskUserQuestionBannerHandle {
  submitCustomAnswer: (text: string) => void;
}

interface Props {
  questions: AskUserQuestionItem[];
  onSubmitAnswers: (answers: QuestionAnswer[]) => void;
  isSubmitting: boolean;
  isTimedOut: boolean;
  requestedAt?: string;
  timeoutAt?: string;
  error: string | null;
}

function toQuestionAnswers(
  questions: AskUserQuestionItem[],
  rec: Record<string, string[]>
): QuestionAnswer[] {
  return questions
    .filter((q) => rec[q.question] !== undefined)
    .map((q) => ({ question: q.question, answer: rec[q.question]! }));
}

function firstUnansweredIndex(
  questions: AskUserQuestionItem[],
  answers: Record<string, string[]>
): number {
  const idx = questions.findIndex((q) => answers[q.question] === undefined);
  return idx === -1 ? questions.length : idx;
}

interface OptionButtonProps {
  label: string;
  description?: string;
  selected: boolean;
  multiSelect: boolean;
  disabled: boolean;
  onSelect: () => void;
}

function OptionButton({
  label,
  description,
  selected,
  multiSelect,
  disabled,
  onSelect,
}: OptionButtonProps) {
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onSelect}
      title={description}
      aria-pressed={multiSelect ? selected : undefined}
      className={cn(
        'group w-full text-left rounded-md border px-3 py-2.5 transition-all',
        'flex items-start gap-2.5',
        selected
          ? 'border-blue-400/70 bg-blue-50/80 dark:bg-blue-950/40 shadow-sm ring-1 ring-blue-400/25'
          : 'border-border/70 bg-background/80 hover:border-blue-400/40 hover:bg-blue-50/40 dark:hover:bg-blue-950/20',
        disabled ? 'opacity-50 cursor-not-allowed' : 'cursor-pointer'
      )}
    >
      <span
        className={cn(
          'mt-0.5 flex h-4 w-4 shrink-0 items-center justify-center rounded border transition-colors',
          multiSelect ? 'rounded-sm' : 'rounded-full',
          selected
            ? 'border-blue-500 bg-blue-500 text-white'
            : 'border-muted-foreground/40 bg-background group-hover:border-blue-400/60'
        )}
        aria-hidden
      >
        {selected && <Check className="h-2.5 w-2.5 stroke-[3]" />}
      </span>
      <span className="min-w-0 flex-1">
        <span
          className={cn(
            'block text-sm font-medium leading-snug',
            selected ? 'text-blue-900 dark:text-blue-100' : 'text-foreground'
          )}
        >
          {label}
        </span>
        {description && (
          <span className="mt-0.5 block text-sm leading-relaxed text-muted-foreground">
            {description}
          </span>
        )}
      </span>
    </button>
  );
}

const AskUserQuestionBanner = forwardRef<AskUserQuestionBannerHandle, Props>(
  function AskUserQuestionBanner(
    {
      questions,
      onSubmitAnswers,
      isSubmitting,
      isTimedOut,
      requestedAt,
      timeoutAt,
      error,
    },
    ref
  ) {
    const { t } = useTranslation('common');
    const customInputId = useId();
    const [answers, setAnswers] = useState<Record<string, string[]>>({});
    const [multiSelectLabels, setMultiSelectLabels] = useState<Set<string>>(
      new Set()
    );
    const [customText, setCustomText] = useState('');
    const hasTimeout = Boolean(timeoutAt);
    const { timeLeft } = useApprovalCountdown(
      requestedAt ?? timeoutAt ?? 0,
      timeoutAt ?? 0,
      isTimedOut || isSubmitting || !hasTimeout
    );
    const isExpired = isTimedOut || (hasTimeout && timeLeft <= 0);

    const questionsKey = useMemo(
      () => questions.map((q) => q.question).join('\0'),
      [questions]
    );

    useEffect(() => {
      setAnswers({});
      setMultiSelectLabels(new Set());
      setCustomText('');
    }, [questionsKey]);

    const currentIndex = useMemo(
      () => firstUnansweredIndex(questions, answers),
      [questions, answers]
    );

    const currentQuestion =
      currentIndex < questions.length ? questions[currentIndex] : null;
    const isAllAnswered = currentIndex >= questions.length;
    const isLastQuestion = currentIndex === questions.length - 1;
    const disabled = isSubmitting || isExpired;
    const totalQuestions = questions.length;

    useEffect(() => {
      setMultiSelectLabels(new Set());
      setCustomText('');
    }, [currentIndex]);

    const commitAnswer = useCallback(
      (labels: string[]) => {
        if (disabled || !currentQuestion) return;

        const trimmed = labels.map((l) => l.trim()).filter(Boolean);
        if (trimmed.length === 0) return;

        const newAnswers = {
          ...answers,
          [currentQuestion.question]: trimmed,
        };
        setAnswers(newAnswers);
        setMultiSelectLabels(new Set());
        setCustomText('');

        if (isLastQuestion) {
          onSubmitAnswers(toQuestionAnswers(questions, newAnswers));
        }
      },
      [
        disabled,
        currentQuestion,
        answers,
        isLastQuestion,
        questions,
        onSubmitAnswers,
      ]
    );

    const handleSelectOption = useCallback(
      (label: string) => {
        if (!currentQuestion?.multiSelect) {
          commitAnswer([label]);
          return;
        }
        setMultiSelectLabels((prev) => {
          const next = new Set(prev);
          if (next.has(label)) {
            next.delete(label);
          } else {
            next.add(label);
          }
          return next;
        });
      },
      [currentQuestion, commitAnswer]
    );

    const handleConfirmMultiSelect = useCallback(() => {
      commitAnswer(Array.from(multiSelectLabels));
    }, [commitAnswer, multiSelectLabels]);

    const handleCustomSubmit = useCallback(() => {
      commitAnswer([customText]);
    }, [commitAnswer, customText]);

    const handleCustomKeyDown = useCallback(
      (e: React.KeyboardEvent<HTMLInputElement>) => {
        if (e.key === 'Enter') {
          e.preventDefault();
          handleCustomSubmit();
        }
      },
      [handleCustomSubmit]
    );

    useImperativeHandle(
      ref,
      () => ({
        submitCustomAnswer: (text: string) => commitAnswer([text]),
      }),
      [commitAnswer]
    );

    if (isAllAnswered && !isSubmitting) return null;

    return (
      <div
        className={cn(
          'overflow-hidden rounded-md border border-blue-400/30 text-sm shadow-sm',
          'border-l-4 border-l-blue-400',
          'bg-blue-50/40 dark:bg-blue-950/15'
        )}
      >
        <div className="flex items-center gap-2.5 border-b border-blue-200/40 px-3 py-2.5 dark:border-blue-800/40">
          <span
            className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-blue-100 dark:bg-blue-900/50"
            aria-hidden
          >
            <MessageCircleQuestion className="h-3.5 w-3.5 text-blue-600 dark:text-blue-300" />
          </span>
          <span className="min-w-0 flex-1 font-medium text-blue-900 dark:text-blue-100">
            {t('conversation.question.askUser.title')}
          </span>
          {hasTimeout && !isExpired && !isSubmitting && (
            <span className="shrink-0 text-xs text-muted-foreground tabular-nums">
              {t('conversation.approvalStatus.timeRemaining', {
                time: formatApprovalTime(timeLeft),
              })}
            </span>
          )}
          {totalQuestions > 1 && (
            <span className="shrink-0 rounded-full bg-blue-100/90 px-2 py-0.5 text-sm font-medium tabular-nums text-blue-700 dark:bg-blue-900/60 dark:text-blue-200">
              {t('conversation.question.askUser.progress', {
                current: Math.min(currentIndex + 1, totalQuestions),
                total: totalQuestions,
              })}
            </span>
          )}
        </div>

        {isExpired && (
          <div className="border-b border-amber-200/50 bg-amber-50/80 px-3 py-2 text-amber-800 dark:border-amber-800/40 dark:bg-amber-950/25 dark:text-amber-200">
            {t('conversation.question.timedOut')}
          </div>
        )}

        {currentQuestion && (
          <div className="px-3 py-2.5" role="group" aria-live="polite">
            <div className="mb-2 flex flex-wrap items-center gap-1.5">
              <span className="rounded border border-blue-200/60 bg-background/60 px-2 py-0.5 text-sm font-medium text-blue-800 dark:border-blue-800/50 dark:text-blue-200">
                {currentQuestion.header}
              </span>
              {currentQuestion.multiSelect && (
                <span className="text-sm text-muted-foreground">
                  {t('conversation.question.askUser.multiSelect')}
                </span>
              )}
            </div>
            <p className="mb-2 font-medium leading-snug text-foreground">
              {currentQuestion.question}
            </p>

            <div className="flex flex-col gap-2">
              {currentQuestion.options.map((opt, idx) => (
                <OptionButton
                  key={`${opt.label}-${idx}`}
                  label={opt.label}
                  description={opt.description}
                  selected={
                    currentQuestion.multiSelect
                      ? multiSelectLabels.has(opt.label)
                      : false
                  }
                  multiSelect={currentQuestion.multiSelect}
                  disabled={disabled}
                  onSelect={() => handleSelectOption(opt.label)}
                />
              ))}
            </div>

            {currentQuestion.multiSelect && multiSelectLabels.size > 0 && (
              <Button
                type="button"
                size="sm"
                disabled={disabled}
                onClick={handleConfirmMultiSelect}
                className="mt-2.5"
              >
                {t('conversation.question.askUser.confirmSelection')}
              </Button>
            )}

            <div className="mt-3 border-t border-blue-200/40 pt-2 dark:border-blue-800/40">
              <label
                htmlFor={customInputId}
                className="mb-1.5 block font-medium text-muted-foreground"
              >
                {t('conversation.question.askUser.other')}
              </label>
              <div className="flex items-center gap-2">
                <input
                  id={customInputId}
                  type="text"
                  value={customText}
                  onChange={(e) => setCustomText(e.target.value)}
                  onKeyDown={handleCustomKeyDown}
                  disabled={disabled}
                  placeholder={t(
                    'conversation.question.askUser.customPlaceholder'
                  )}
                  className={cn(
                    'min-w-0 flex-1 rounded-md border border-input/80 bg-background px-2.5 py-1.5',
                    'placeholder:text-muted-foreground/70',
                    'focus:outline-none focus:ring-1 focus:ring-blue-400/30 focus:border-blue-400/50',
                    'disabled:cursor-not-allowed disabled:opacity-50'
                  )}
                />
                {customText.trim() && (
                  <Button
                    type="button"
                    size="sm"
                    disabled={disabled}
                    onClick={handleCustomSubmit}
                    className="shrink-0"
                  >
                    {t('conversation.question.askUser.submit')}
                  </Button>
                )}
              </div>
            </div>
          </div>
        )}

        {error && (
          <div
            className="border-t border-destructive/20 bg-destructive/5 px-3 py-2 text-destructive"
            role="alert"
          >
            {error}
          </div>
        )}

        {isSubmitting && (
          <div className="flex items-center gap-2 border-t border-blue-200/40 px-3 py-2.5 text-muted-foreground dark:border-blue-800/40">
            <Loader2
              className="h-3.5 w-3.5 animate-spin text-blue-600 dark:text-blue-400"
              aria-hidden
            />
            {t('conversation.question.askUser.submitting')}
          </div>
        )}
      </div>
    );
  }
);

export default AskUserQuestionBanner;
