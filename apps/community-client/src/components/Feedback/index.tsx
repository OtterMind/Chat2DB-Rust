import { useState } from 'react';
import { IconButton, IconfontSvg, Input, Modal, TextArea } from '@chat2db/ui';
import { useStyles } from './style';
import { Image, Button, Checkbox, Flex, Tooltip } from 'antd';

import Terrible from '@/assets/img/feedback/Terrible.svg';
import Bad from '@/assets/img/feedback/Bad.svg';
import Okay from '@/assets/img/feedback/Okay.svg';
import Good from '@/assets/img/feedback/Good.svg';
import Amazing from '@/assets/img/feedback/Amazing.svg';
import Upload from '@/components/Upload';
import miscService from '@/service/misc';
import { UploadTypeEnum } from '@/typings/upload';
import i18n from '@/i18n';
import { isDesktop } from '@/utils/env';
import feedback from '@/utils/feedback';

const emailRegex = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;
const phoneRegex = /^\+?\d{10,15}$/; // Simple phone-number pattern; adjust as requirements change.

const faceEmoji = [
  {
    code: 'Terrible',
    url: Terrible,
    title: i18n('feedback.feeling.terrible'),
  },
  {
    code: 'Bad',
    url: Bad,
    title: i18n('feedback.feeling.bad'),
  },
  {
    code: 'Okay',
    url: Okay,
    title: i18n('feedback.feeling.ok'),
  },
  {
    code: 'Good',
    url: Good,
    title: i18n('feedback.feeling.good'),
  },
  {
    code: 'Amazing',
    url: Amazing,
    title: i18n('feedback.feeling.amazing'),
  },
];

const improveList = [
  {
    type: 'AI',
    icon: 'icon-sparkles',
    title: i18n('feedback.improve.ai'),
  },
  {
    type: 'User Interface',
    icon: 'icon-edit',
    title: i18n('feedback.improve.ui'),
  },
  {
    type: 'Function Suggestions',
    icon: 'icon-formatting',
    title: i18n('feedback.improve.function'),
  },
];

const imageCount = 5;

const getBase64 = (file) =>
  new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.readAsDataURL(file);
    reader.onload = () => resolve(reader.result);
    reader.onerror = (error) => reject(error);
  });

interface FeedbackProps {
  /** Externally controlled modal state; controlled mode does not render IconButton. */
  open?: boolean;
  /** Externally controlled close callback. */
  onClose?: () => void;
}

const Feedback = ({ open: externalOpen, onClose }: FeedbackProps = {}) => {
  const [internalOpen, setInternalOpen] = useState(false);
  const modalOpen = externalOpen !== undefined ? externalOpen : internalOpen;
  const { styles, cx } = useStyles();

  const [contactInfo, setContactInfo] = useState<string>('');
  const [feeling, setFeeling] = useState('Amazing');
  const [improveType, setImproveType] = useState('');
  const [feedbackContent, setFeedbackContent] = useState('');
  const [imageList, setImageList] = useState([]);
  const [previewImage, setPreviewImage] = useState('');
  const [previewOpen, setPreviewOpen] = useState(false);
  const [uploadLog, setUploadLog] = useState(true);

  const beforeUpload = (file) => {
    // Limit images to 2 MB.
    const isLt2M = file.size / 1024 / 1024 < 2;
    if (!isLt2M) {
      feedback.error(i18n('feedback.validate.upload.image'));
    }
    return isLt2M;
  };

  const handleChange = ({ fileList: newFileList }) => {
    setImageList(newFileList);
  };

  const handlePreview = async (file) => {
    if (!file.url && !file.preview) {
      file.preview = await getBase64(file.originFileObj);
    }
    setPreviewImage(file.url || file.preview);
    setPreviewOpen(true);
  };

  const validate = () => {
    if (!contactInfo) {
      feedback.error(i18n('feedback.validate.contactInfo'));
      return false;
    }
    if (!feeling) {
      feedback.error(i18n('feedback.validate.feeling'));
      return false;
    }
    if (!improveType) {
      feedback.error(i18n('feedback.validate.improve'));
      return false;
    }
    if (!feedbackContent) {
      feedback.error(i18n('feedback.validate.content'));
      return false;
    }
    return true;
  };

  const handleSubmit = async () => {
    if (!validate()) return;

    let params = {};
    // Determine whether contactInfo is an email, WeChat ID, or phone number.
    if (emailRegex.test(contactInfo)) {
      params = {
        email: contactInfo,
      };
    } else if (phoneRegex.test(contactInfo)) {
      params = {
        phone: contactInfo,
      };
    } else {
      // WeChat.
      params = {
        wechatId: contactInfo,
      };
    }

    // Sentiment.
    params['review'] = feeling;

    // feedbackType
    params['feedbackType'] = improveList.find((item) => item.type === improveType)?.title;

    // Description.
    params['description'] = feedbackContent;

    // Images.
    const imageUrls = imageList.map((item: any) => {
      if (item?.response.res.status === 200) {
        return item?.response.privateUrl;
      }
    });
    params['imageUrls'] = imageUrls;

    // Upload logs.
    params['uploadLog'] = uploadLog;

    await miscService.createFeedback(params);
    feedback.success(i18n('feedback.submit.success'));
    handleCancel();
  };

  const handleCancel = () => {
    if (onClose) onClose();
    setInternalOpen(false);
    setContactInfo('');
    setFeeling('Amazing');
    setImproveType('');
    setFeedbackContent('');
  };

  return (
    <>
      {externalOpen === undefined && (
        <IconButton
          key="feedback"
          code="icon-annotation"
          tooltipPlacement="right"
          onClick={() => {
            setInternalOpen(true);
          }}
        />
      )}
      <Modal
        centered
        open={modalOpen}
        maxHeight={'95vh'}
        onCancel={handleCancel}
        padding={0}
        mask={false}
        maskClosable={false}
        destroyOnClose
        footer={
          <div className={styles.footer}>
            <Button size="large" onClick={handleCancel}>
              {i18n('common.button.cancel')}
            </Button>
            <Button size="large" type="primary" onClick={handleSubmit}>
              {i18n('common.button.submit')}
            </Button>
          </div>
        }
      >
        <div className={styles.wrapper}>
          <div className={styles.titleWrapper}>
            <div className={styles.title}>
              <IconfontSvg code="icon-annotation" /> {i18n('feedback.title')}
            </div>
            <div className={styles.description}>{i18n('feedback.subtitle')}</div>
          </div>

          <div className={styles.contentWrapper}>
            <div className={styles.block}>
              <div className={styles.segTitle}>{i18n('feedback.contactInfo')}</div>
              <Input
                type="ghost"
                placeholder={i18n('feedback.contactInfo.placeholder')}
                value={contactInfo}
                onChange={(v) => setContactInfo(v.target.value)}
              />
            </div>

            <div className={styles.block}>
              <div className={styles.segTitle}>{i18n('feedback.feeling')}</div>
              <Flex gap={24}>
                {faceEmoji.map((item, index) => (
                  <Tooltip key={index} title={item.title} placement="top">
                    <div
                      className={cx(styles.emoji, feeling === item.code && styles.emojiActive)}
                      onClick={() => {
                        setFeeling(item.code);
                      }}
                    >
                      <img src={item.url} alt={item.code} />
                    </div>
                  </Tooltip>
                ))}
              </Flex>
            </div>

            <div className={styles.block}>
              <div className={styles.segTitle}>{i18n('feedback.improve')}</div>
              <Flex gap={8}>
                {improveList.map((item, index) => (
                  <div
                    key={index}
                    className={cx(styles.improveBlock, improveType === item.type && styles.improveBlockActive)}
                    onClick={() => setImproveType(item.type)}
                  >
                    <IconfontSvg code={item.icon} size={'sm'} />
                    <div>{item.title}</div>
                  </div>
                ))}
              </Flex>
              <TextArea
                className={styles.textArea}
                value={feedbackContent}
                onChange={(v) => setFeedbackContent(v.target.value)}
                maxLength={500}
                placeholder={i18n('feedback.content')}
                autoSize={{ minRows: 4, maxRows: 8 }}
              />

              <Upload
                accept="image/*"
                maxCount={imageCount}
                listType="picture-card"
                beforeUpload={beforeUpload}
                onChange={handleChange}
                onPreview={handlePreview}
                uploadType={UploadTypeEnum.FEEDBACK_IMG}
              >
                {imageList.length < imageCount && (
                  <div>
                    <IconfontSvg code="icon-upload" size="lg" />
                    <div>{i18n('common.button.upload')}</div>
                  </div>
                )}
              </Upload>
              <div className={styles.imageTips}>{i18n('feedback.upload.tip')}</div>

              {previewImage && (
                <Image
                  wrapperStyle={{
                    display: 'none',
                  }}
                  preview={{
                    visible: previewOpen,
                    onVisibleChange: (visible) => setPreviewOpen(visible),
                    afterOpenChange: (visible) => !visible && setPreviewImage(''),
                  }}
                  src={previewImage}
                />
              )}

              {isDesktop ? (
                <Checkbox checked={uploadLog} onClick={() => setUploadLog(!uploadLog)}>
                  <div className={styles.checkbox}>{i18n('feedback.upload.log')}</div>
                </Checkbox>
              ) : null}
            </div>
          </div>
        </div>
      </Modal>
    </>
  );
};

export default Feedback;
