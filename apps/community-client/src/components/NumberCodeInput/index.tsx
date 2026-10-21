import { useEffect, useRef, useState } from 'react';
import { useStyles } from './style';
import { Flex, Input, type InputRef } from 'antd';

interface IProps {
  onChange?: (value: string) => void;
  onFinished?: (value: string) => void;
  value?: string;
  numberCnt?: number;
}

const NumberCodeInput = ({ onChange, onFinished, value = '', numberCnt = 8 }: IProps) => {
  const [codes, setCodes] = useState(new Array(numberCnt).fill('')); // Keep the array length aligned with numberCnt.
  const inputRefs = useRef<(InputRef | null)[]>([]);
  const containerRef = useRef(null);
  const { styles } = useStyles();

  useEffect(() => {
    if (value) {
      setCodes(value.split('').concat(new Array(numberCnt - value.length).fill('')));
    }
  }, [value]);

  const handleChange = (index, e) => {
    const inputValue = e.target.value;
    const isValid = /^[a-zA-Z0-9]$/.test(inputValue); // Allow only ASCII letters and digits.

    if (!isValid && inputValue !== '') {
      return; // Return immediately for invalid input.
    }

    const newCodes = [...codes];
    newCodes[index] = e.target.value;
    setCodes(newCodes);

    if (e.target.value && index < numberCnt - 1) {
      inputRefs.current[index + 1]?.focus();
    }

    const newValue = newCodes.join('');
    if (onChange) {
      onChange(newValue);
    }

    if (newValue.length === numberCnt) {
      // Keep this aligned with the configured code length.
      console.log('Team code completed:', newValue);
      onFinished && onFinished(newValue);
    }
  };

  const handleKeyDown = (index, e) => {
    if (e.key === 'Backspace' && !codes[index] && index > 0) {
      inputRefs.current[index - 1]?.focus();
    }
  };
  const handlePaste = (e) => {
    e.preventDefault();
    const pasteData = e.clipboardData.getData('Text');
    const validPasteData = pasteData.replace(/[^a-zA-Z0-9]/g, ''); // Keep only ASCII letters and digits.

    const newCodes = [...codes];
    let currentIndex = codes.findIndex((code) => code === '');

    if (currentIndex === -1) {
      currentIndex = numberCnt; // Start from the end when no empty slot remains.
    }

    for (let i = 0; i < validPasteData.length && currentIndex < numberCnt; i++) {
      newCodes[currentIndex] = validPasteData[i];
      currentIndex++;
    }

    setCodes(newCodes);

    const newValue = newCodes.join('');
    if (onChange) {
      onChange && onChange(newValue);
    }

    if (newValue.length === numberCnt) {
      onFinished && onFinished(newValue);
    }

    hanldeMoveCursor(newCodes);
  };

  const focusFirstEmptyInput = (e) => {
    e.preventDefault(); // Prevent default focus behavior.

    hanldeMoveCursor(codes);
  };

  const hanldeMoveCursor = (nextCodes) => {
    const firstEmptyIndex = nextCodes.findIndex((code) => code === '');
    if (firstEmptyIndex !== -1) {
      inputRefs.current[firstEmptyIndex]?.focus();
    } else {
      inputRefs.current[numberCnt - 1]?.focus();
    }
  };

  return (
    <Flex justify="space-between" ref={containerRef} onClick={focusFirstEmptyInput} style={{ cursor: 'text' }}>
      {codes.map((code, index) => (
        <Input
          key={index}
          ref={(el) => (inputRefs.current[index] = el)}
          value={code}
          onChange={(e) => handleChange(index, e)}
          onKeyDown={(e) => handleKeyDown(index, e)}
          onPaste={(e) => handlePaste(e)} // Handle pasted codes.
          maxLength={1}
          // style={{ width: 40, height: '52px', textAlign: 'center' }}
          className={styles.input}
          onMouseDown={(e) => e.preventDefault()} // Prevent pointer clicks from focusing the input.
          // onFocus={(e) => e.target.blur()} // Blur immediately if the input somehow gains focus.
        />
      ))}
    </Flex>
  );
};

export default NumberCodeInput;
